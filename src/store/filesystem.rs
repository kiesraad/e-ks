//! Filesystem-backed persistence for the event store.
//!
//! Events are stored as length-prefixed postcard frames in a single file per stream.
//! The length prefix lets us read one frame at a time without decoding the whole file,
//! and lets us append new frames at the end. Appends use `O_APPEND` to avoid rewriting.
//!
//! On-disk layout per frame:
//!   [4 bytes: body length (u32, little-endian)]
//!   [body:    postcard-encoded [`Frame`]]
//!
//! [`Frame`] is a versioned enum; postcard encodes the variant discriminant as a
//! varint, giving us a per-frame version marker for future format migrations.

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
};

use super::{
    EncryptedEvent, EventHash, Replay, Store, StoreData, StreamMeta, chain_hash, event_aad,
    persistence::NewStream,
};
use crate::{
    AppError, ElectionConfig, Scope, StreamId,
    crypto::{EventCipher, WrappedKey},
};

const FRAME_HEADER_LEN: usize = 4;

#[derive(Serialize, Deserialize)]
enum Frame {
    V1 {
        event_id: u64,
        created_at_micros: i64,
        /// Chain hash; see [`super::chain_hash`] and [`super::StoreEvent::hash`].
        hash: EventHash,
        encrypted_payload: Vec<u8>,
    },
}

/// Ensure the filesystem storage directory exists.
pub async fn init_local(dir: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dir).await.map_err(AppError::ServerError)
}

/// Replay the stream file into `store`, returning the last event id present in
/// the file (which is ahead of the projection when the replay was truncated;
/// see [`Replay`]) alongside the replay outcome.
pub async fn replay_from_file<D>(
    store: &Store<D>,
    dir: &Path,
    cipher: &EventCipher,
) -> Result<(usize, Replay), AppError>
where
    D: StoreData,
    D::Event: DeserializeOwned,
{
    let path = stream_path(dir, store.stream_id, store.election);
    let frames = read_frames(&path).await?;
    let last_file_id = frames.iter().map(|f| f.event_id).max().unwrap_or(0);

    let mut data = store.data.write();
    let replay = super::apply_encrypted_events(&mut *data, cipher, frames)?;

    if let Some(event_id) = replay.truncated_at {
        tracing::error!(
            stream_id = %store.stream_id,
            election = %store.election.stable_id(),
            event_id,
            "stream file is only partially readable by this build; \
             serving the projection as of the preceding event"
        );
    }

    Ok((last_file_id, replay))
}

/// List the elections that have persisted events under the given stream.
pub async fn elections_for_stream(dir: &Path, stream_id: StreamId) -> Vec<ElectionConfig> {
    let mut result = Vec::new();
    visit_non_empty_stream_files(dir, |id, election| {
        if id == stream_id {
            result.push(election);
        }
    })
    .await;
    result
}

/// Iterate non-empty `{stream_id}_{election}.bin` files in `dir`, invoking
/// `callback` for each successfully parsed entry. Directory read errors and
/// unparsable filenames are ignored.
async fn visit_non_empty_stream_files(
    dir: &Path,
    mut callback: impl FnMut(StreamId, ElectionConfig),
) {
    let Ok(mut entries) = fs::read_dir(dir).await else {
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let Some((stream_id, election)) = parse_stream_filename(&entry.file_name()) else {
            continue;
        };
        if let Ok(meta) = entry.metadata().await
            && meta.len() > 0
        {
            callback(stream_id, election);
        }
    }
}

/// Parse a filename of the form `{uuid}_{election_stable_id}.bin` back into its
/// `(stream_id, election)` parts. Returns `None` for unrelated files.
fn parse_stream_filename(file_name: &std::ffi::OsStr) -> Option<(StreamId, ElectionConfig)> {
    let name = file_name.to_str()?;
    let stem = name.strip_suffix(".bin")?;
    let (id_str, election_segment) = stem.split_once('_')?;
    let stream_id = StreamId::from_str(id_str).ok()?;
    // Undo the filename-safe `:` -> `_` mangling (see `stream_path`).
    let election = ElectionConfig::from_stable_id(&election_segment.replacen('_', ":", 1))?;
    Some((stream_id, election))
}

/// Ensure a stream file and its wrapped key sidecar exist, returning the
/// stored wrapped key.
///
/// File storage only ever holds [`Scope::PoliticalGroup`] streams (CSB data
/// lives only in the database backend), so a stream of any other scope is
/// rejected here rather than persisted. Because every store is created through
/// [`super::persistence::StorePersistence`] before its first write, this is
/// the single gate that keeps non-political-group events off disk.
pub async fn ensure_stream_file(dir: &Path, new: &NewStream) -> Result<WrappedKey, AppError> {
    if new.scope != Scope::PoliticalGroup {
        return Err(AppError::ConfigLoadError(format!(
            "local file storage only supports political-group streams, \
             got scope `{}`; use database storage for CSB",
            new.scope.as_str()
        )));
    }

    let path = stream_path(dir, new.stream_id, new.election);
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .await
        .map_err(AppError::ServerError)?;

    load_or_create_key_file(dir, new).await
}

/// Read the stream's `.key` sidecar, writing `new`'s wrapped key first if the
/// file is missing.
async fn load_or_create_key_file(dir: &Path, new: &NewStream) -> Result<WrappedKey, AppError> {
    let path = key_path(dir, new.stream_id, new.election);

    match fs::read(&path).await {
        Ok(wrapped) => Ok(WrappedKey::from(wrapped)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // create_new: never clobber a concurrently created key
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .await
            {
                Ok(mut file) => {
                    file.write_all(new.encrypted_key.as_bytes())
                        .await
                        .map_err(AppError::ServerError)?;
                    file.sync_data().await.map_err(AppError::ServerError)?;
                    Ok(new.encrypted_key.clone())
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    // lost the race: use the winner's key
                    fs::read(&path)
                        .await
                        .map(WrappedKey::from)
                        .map_err(AppError::ServerError)
                }
                Err(err) => Err(AppError::ServerError(err)),
            }
        }
        Err(err) => Err(AppError::ServerError(err)),
    }
}

/// List every `(stream_id, election)` stream with the given scope.
///
/// All on-disk streams are [`Scope::PoliticalGroup`] (enforced by
/// [`ensure_stream_file`]), so a political-group query returns every non-empty
/// stream and any other scope returns nothing.
pub async fn streams_by_scope(dir: &Path, scope: Scope) -> Vec<(StreamId, ElectionConfig)> {
    if scope != Scope::PoliticalGroup {
        return Vec::new();
    }

    let mut result = Vec::new();
    visit_non_empty_stream_files(dir, |stream_id, election| {
        result.push((stream_id, election));
    })
    .await;
    result
}

/// Locate the event whose chain hash begins with `hash_prefix`, returning its
/// `(stream_id, election, event_id)`.
///
/// Every on-disk stream is a political-group stream, so this scans them all,
/// mirroring the database lookup, which restricts itself to political-group
/// scope. An ambiguous prefix matching more than one event is reported as
/// [`AppError::AmbiguousHash`].
pub async fn find_event_by_hash_prefix(
    dir: &Path,
    hash_prefix: &[u8],
) -> Result<Option<(StreamId, ElectionConfig, usize)>, AppError> {
    let mut streams = Vec::new();
    visit_non_empty_stream_files(dir, |stream_id, election| {
        streams.push((stream_id, election));
    })
    .await;

    let mut matches = Vec::new();
    for (stream_id, election) in streams {
        let path = stream_path(dir, stream_id, election);
        for EncryptedEvent { event_id, hash, .. } in read_frames(&path).await? {
            if hash.starts_with(hash_prefix) {
                matches.push((stream_id, election, event_id));
                if matches.len() > 1 {
                    return Err(AppError::AmbiguousHash);
                }
            }
        }
    }

    Ok(matches.into_iter().next())
}

/// List [`StreamMeta`] for every non-empty stream with the given scope. All
/// on-disk streams are [`Scope::PoliticalGroup`], so other scopes yield nothing.
pub async fn stream_metadata_by_scope(
    dir: &Path,
    scope: Scope,
) -> Result<Vec<StreamMeta>, AppError> {
    if scope != Scope::PoliticalGroup {
        return Ok(Vec::new());
    }

    let mut streams = Vec::new();
    visit_non_empty_stream_files(dir, |stream_id, election| {
        streams.push((stream_id, election));
    })
    .await;

    let mut result = Vec::with_capacity(streams.len());
    for (stream_id, election) in streams {
        let path = stream_path(dir, stream_id, election);
        let frames = read_frames(&path).await?;
        result.push(StreamMeta {
            stream_id,
            election,
            event_count: frames.iter().map(|f| f.event_id).max().unwrap_or(0),
            created_at: frames.first().map(|f| f.created_at),
            last_event_at: frames.last().map(|f| f.created_at),
        });
    }

    Ok(result)
}

/// Read each frame's `(event_id, created_at, chain hash, encrypted payload)`
/// from a stream file without decrypting. Empty vector if the file does not exist.
async fn read_frames(path: &Path) -> Result<Vec<EncryptedEvent>, AppError> {
    let mut file = match File::open(path).await {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(AppError::ServerError(err)),
    };

    let mut result = Vec::new();
    loop {
        let mut len_buf = [0u8; FRAME_HEADER_LEN];
        match file.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(AppError::ServerError(err)),
        }
        let body_len = u32::from_le_bytes(len_buf) as usize;

        let mut body = vec![0u8; body_len];
        file.read_exact(&mut body)
            .await
            .map_err(AppError::ServerError)?;

        let Frame::V1 {
            event_id,
            created_at_micros,
            hash,
            encrypted_payload,
        } = postcard::from_bytes::<Frame>(&body)
            .map_err(|e| AppError::EventDecodeError(format!("failed to decode frame: {e}")))?;

        let created_at = DateTime::from_timestamp_micros(created_at_micros).unwrap_or_default();
        result.push(EncryptedEvent {
            event_id: event_id as usize,
            created_at,
            hash,
            payload: encrypted_payload,
        });
    }

    Ok(result)
}

/// Encrypt `payload`, append a frame for it to the stream file, and return the
/// event's chain hash (computed over the encrypted blob).
pub(super) async fn append_event<E: Serialize>(
    path: &Path,
    cipher: &EventCipher,
    event_id: usize,
    created_at: DateTime<Utc>,
    payload: &E,
    prev_hash: &EventHash,
) -> Result<[u8; 32], AppError> {
    let aad = event_aad(event_id, created_at, prev_hash);
    let encrypted_payload = cipher.encrypt(payload, &aad)?;
    let hash = chain_hash(prev_hash, event_id, created_at, &encrypted_payload);

    let frame = Frame::V1 {
        event_id: event_id as u64,
        created_at_micros: created_at.timestamp_micros(),
        hash,
        encrypted_payload,
    };

    let body = postcard::to_allocvec(&frame).map_err(|e| {
        AppError::ServerError(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;

    let body_len = u32::try_from(body.len()).map_err(|_| {
        AppError::ServerError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame body exceeds u32::MAX",
        ))
    })?;

    let mut buf = Vec::with_capacity(FRAME_HEADER_LEN + body.len());
    buf.extend_from_slice(&body_len.to_le_bytes());
    buf.extend_from_slice(&body);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(AppError::ServerError)?;

    let written = file.write(&buf).await.map_err(AppError::ServerError)?;
    if written != buf.len() {
        return Err(AppError::ServerError(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "partial filesystem append",
        )));
    }

    file.sync_data().await.map_err(AppError::ServerError)?;

    Ok(hash)
}

pub(super) fn stream_path(dir: &Path, stream_id: StreamId, election: ElectionConfig) -> PathBuf {
    stream_file(dir, stream_id, election, "bin")
}

/// Sidecar file holding the stream's wrapped encryption key.
fn key_path(dir: &Path, stream_id: StreamId, election: ElectionConfig) -> PathBuf {
    stream_file(dir, stream_id, election, "key")
}

fn stream_file(dir: &Path, stream_id: StreamId, election: ElectionConfig, ext: &str) -> PathBuf {
    // Election identifiers can contain ':' (e.g. `PS27:prov1`); replace with '_'
    // so the filename stays portable on all filesystems.
    let election_segment = election.stable_id().replace(':', "_");
    dir.join(format!("{stream_id}_{election_segment}.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Event,
        crypto::{MasterKey, StreamKey},
        store::{GENESIS_HASH, StoreEvent},
    };
    use parking_lot::RwLock;
    use secrecy::SecretString;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestEvent {
        label: String,
    }

    impl Event for TestEvent {
        fn category(&self) -> &'static str {
            "test_event"
        }

        fn key(&self) -> &'static str {
            "test_event"
        }

        fn description(&self, _locale: crate::Locale) -> String {
            self.label.to_string()
        }

        fn details(&self) -> String {
            self.label.to_string()
        }
    }

    #[derive(Default)]
    struct TestData {
        events: Vec<StoreEvent<TestEvent>>,
    }

    impl StoreData for TestData {
        type Event = TestEvent;

        fn apply(&mut self, event: StoreEvent<Self::Event>) {
            self.events.push(event);
        }

        fn events(&self) -> &[StoreEvent<Self::Event>] {
            &self.events
        }

        fn scope() -> Scope {
            Scope::PoliticalGroup
        }
    }

    /// The same event after a field was added to it: what this build expects to
    /// decode when the stored bytes predate the change.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct WiderTestEvent {
        label: String,
        extra: u32,
    }

    impl Event for WiderTestEvent {
        fn category(&self) -> &'static str {
            "test_event"
        }

        fn key(&self) -> &'static str {
            "test_event"
        }

        fn description(&self, _locale: crate::Locale) -> String {
            self.label.to_string()
        }

        fn details(&self) -> String {
            self.label.to_string()
        }
    }

    #[derive(Default)]
    struct WiderTestData {
        events: Vec<StoreEvent<WiderTestEvent>>,
    }

    impl StoreData for WiderTestData {
        type Event = WiderTestEvent;

        fn apply(&mut self, event: StoreEvent<Self::Event>) {
            self.events.push(event);
        }

        fn events(&self) -> &[StoreEvent<Self::Event>] {
            &self.events
        }

        fn scope() -> Scope {
            Scope::PoliticalGroup
        }
    }

    #[test]
    fn test_event_trait_impl() {
        let event = TestEvent {
            label: "hello".to_string(),
        };
        assert_eq!(event.category(), "test_event");
        assert_eq!(event.key(), "test_event");
        assert_eq!(event.description(crate::Locale::En), "hello");
        assert_eq!(event.details(), "hello");
    }

    fn test_master() -> MasterKey {
        MasterKey::new(&SecretString::from("test-secret"))
    }

    const TEST_ELECTION: ElectionConfig = ElectionConfig::EK27;

    fn test_new_stream(stream_id: StreamId, scope: Scope, master: &MasterKey) -> NewStream {
        let key = StreamKey::generate();
        NewStream {
            stream_id,
            election: TEST_ELECTION,
            scope,
            encrypted_key: master.wrap_key(&key, stream_id, TEST_ELECTION).unwrap(),
        }
    }

    async fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("eks-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).await.expect("create temp dir");
        dir
    }

    fn test_store(stream_id: StreamId) -> Store<TestData> {
        Store {
            stream_id,
            election: TEST_ELECTION,
            backend: crate::store::StoreBackend::Memory {
                store: super::super::memory::MemoryStore::default(),
            },
            data: Arc::new(RwLock::new(TestData::default())),
        }
    }

    /// A fresh stream cipher, as a real store would unwrap from its key file.
    fn test_cipher() -> EventCipher {
        StreamKey::generate().cipher()
    }

    /// A store wired to the local backend, as the registry would build it.
    fn local_store_for<D: StoreData>(
        dir: &Path,
        stream_id: StreamId,
        cipher: &EventCipher,
    ) -> Store<D> {
        Store {
            stream_id,
            election: TEST_ELECTION,
            backend: crate::store::StoreBackend::Local {
                dir: dir.to_path_buf(),
                cipher: Box::new(cipher.clone()),
            },
            data: Arc::new(RwLock::new(D::default())),
        }
    }

    fn local_store(dir: &Path, stream_id: StreamId, cipher: &EventCipher) -> Store<TestData> {
        local_store_for(dir, stream_id, cipher)
    }

    #[tokio::test]
    async fn init_local_creates_directory() {
        let dir = temp_dir().await.join("nested");
        init_local(&dir).await.expect("init local");
        assert!(dir.exists());
    }

    #[tokio::test]
    async fn update_and_load_replays_events() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let cipher = test_cipher();
        let store = local_store(&dir, stream_id, &cipher);
        store
            .update(TestEvent {
                label: "first".to_string(),
            })
            .await?;
        store
            .update(TestEvent {
                label: "second".to_string(),
            })
            .await?;

        let path = stream_path(&dir, store.stream_id, TEST_ELECTION);
        let file_contents = fs::read(&path).await.expect("read log");
        // Binary file should not be empty
        assert!(!file_contents.is_empty());

        let fresh = test_store(stream_id);
        replay_from_file(&fresh, &dir, &cipher).await?;

        let data = fresh.data.read();
        assert_eq!(data.last_event_id(), 2);
        assert_ne!(data.last_event_hash(), GENESIS_HASH);
        let applied: Vec<(usize, TestEvent)> = data
            .events
            .iter()
            .map(|e| (e.event_id, e.payload.clone()))
            .collect();
        assert_eq!(
            applied,
            vec![
                (
                    1,
                    TestEvent {
                        label: "first".to_string()
                    }
                ),
                (
                    2,
                    TestEvent {
                        label: "second".to_string()
                    }
                ),
            ]
        );

        Ok(())
    }

    #[tokio::test]
    async fn tampering_with_a_stored_event_breaks_the_chain() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let cipher = test_cipher();
        let store = local_store(&dir, stream_id, &cipher);
        store
            .update(TestEvent {
                label: "one".to_string(),
            })
            .await?;
        store
            .update(TestEvent {
                label: "two".to_string(),
            })
            .await?;

        // Flip the last byte of the first frame (the GCM tag of event 1).
        let path = stream_path(&dir, stream_id, TEST_ELECTION);
        let mut bytes = fs::read(&path).await.map_err(AppError::ServerError)?;
        let frame1_len = u32::from_le_bytes(bytes[..FRAME_HEADER_LEN].try_into().unwrap()) as usize;
        let last_byte = FRAME_HEADER_LEN + frame1_len - 1;
        bytes[last_byte] ^= 0x01;
        fs::write(&path, &bytes)
            .await
            .map_err(AppError::ServerError)?;

        let fresh = test_store(stream_id);
        let err = replay_from_file(&fresh, &dir, &cipher)
            .await
            .expect_err("tampering must be detected");
        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert!(fresh.data.read().events.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn an_event_this_build_cannot_decode_truncates_the_replay() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let cipher = test_cipher();
        let path = stream_path(&dir, stream_id, TEST_ELECTION);

        // Event 1 matches this build's event type; event 2 was written before
        // `extra` was added to it, so its payload decodes short.
        let created_at = Utc::now();
        let hash1 = append_event(
            &path,
            &cipher,
            1,
            created_at,
            &WiderTestEvent {
                label: "kept".to_string(),
                extra: 7,
            },
            &GENESIS_HASH,
        )
        .await?;
        let hash2 = append_event(
            &path,
            &cipher,
            2,
            created_at,
            &TestEvent {
                label: "unreadable".to_string(),
            },
            &hash1,
        )
        .await?;

        let store: Store<WiderTestData> = local_store_for(&dir, stream_id, &cipher);
        let (last_file_id, replay) = replay_from_file(&store, &dir, &cipher).await?;

        assert_eq!(last_file_id, 2);
        assert_eq!(replay.truncated_at, Some(2));
        // The chain tip is the last stored event, not the last applied one.
        assert_eq!(replay.chain_tip, hash2);
        assert_eq!(store.data.read().last_event_id(), 1);
        assert_eq!(store.data.read().last_event_hash(), hash1);

        // Reading degraded gracefully, but appending must not: the new event
        // would sit behind the gap and never be applied again.
        let err = store
            .update(WiderTestEvent {
                label: "rejected".to_string(),
                extra: 1,
            })
            .await
            .expect_err("appending to a truncated stream must fail");
        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert_eq!(read_frames(&path).await?.len(), 2);

        Ok(())
    }

    #[cfg(feature = "verify-event-hash-chain")]
    #[tokio::test]
    async fn rewriting_an_event_hash_is_detected() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let cipher = test_cipher();
        let store = local_store(&dir, stream_id, &cipher);
        store
            .update(TestEvent {
                label: "one".to_string(),
            })
            .await?;

        // Rewrite the stored chain hash. AES-GCM does not cover it, so only the
        // chain check can catch this; re-encode the frame so it stays well-formed.
        let path = stream_path(&dir, stream_id, TEST_ELECTION);
        let bytes = fs::read(&path).await.map_err(AppError::ServerError)?;
        let body_len = u32::from_le_bytes(bytes[..FRAME_HEADER_LEN].try_into().unwrap()) as usize;
        let Frame::V1 {
            event_id,
            created_at_micros,
            mut hash,
            encrypted_payload,
        } = postcard::from_bytes::<Frame>(&bytes[FRAME_HEADER_LEN..FRAME_HEADER_LEN + body_len])
            .expect("decode frame");
        hash[0] ^= 0x01;
        let new_body = postcard::to_allocvec(&Frame::V1 {
            event_id,
            created_at_micros,
            hash,
            encrypted_payload,
        })
        .expect("encode frame");
        let mut new_bytes = Vec::new();
        new_bytes.extend_from_slice(&(new_body.len() as u32).to_le_bytes());
        new_bytes.extend_from_slice(&new_body);
        new_bytes.extend_from_slice(&bytes[FRAME_HEADER_LEN + body_len..]);
        fs::write(&path, &new_bytes)
            .await
            .map_err(AppError::ServerError)?;

        let fresh = test_store(stream_id);
        let err = replay_from_file(&fresh, &dir, &cipher)
            .await
            .expect_err("rewritten hash must be detected");
        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert!(fresh.data.read().events.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn ensure_stream_creates_empty_file_and_key() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let master = test_master();
        let stream_id = StreamId::new();
        let new = test_new_stream(stream_id, Scope::PoliticalGroup, &master);
        ensure_stream_file(&dir, &new).await?;

        let path = stream_path(&dir, stream_id, TEST_ELECTION);
        let metadata = fs::metadata(&path).await.map_err(AppError::ServerError)?;
        assert_eq!(metadata.len(), 0);

        // the key sidecar exists and unwraps with the same master key
        let wrapped = fs::read(key_path(&dir, stream_id, TEST_ELECTION))
            .await
            .map_err(AppError::ServerError)?;
        assert!(
            master
                .unwrap_key(&WrappedKey::from(wrapped), stream_id, TEST_ELECTION)
                .is_ok()
        );

        Ok(())
    }

    #[tokio::test]
    async fn ensure_stream_reuses_existing_key() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let master = test_master();
        let stream_id = StreamId::new();
        let first = ensure_stream_file(
            &dir,
            &test_new_stream(stream_id, Scope::PoliticalGroup, &master),
        )
        .await?;
        let second = ensure_stream_file(
            &dir,
            &test_new_stream(stream_id, Scope::PoliticalGroup, &master),
        )
        .await?;

        // the second call returns the stored key, not its own fresh one
        assert_eq!(first, second);

        Ok(())
    }

    #[tokio::test]
    async fn ensure_stream_rejects_non_political_group_scope() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let new = test_new_stream(stream_id, Scope::CentralElectoralCommittee, &test_master());
        let err = ensure_stream_file(&dir, &new)
            .await
            .expect_err("non-political-group scope must be rejected on file storage");
        assert!(matches!(err, AppError::ConfigLoadError(_)));

        // nothing was written: neither stream file nor key
        let path = stream_path(&dir, stream_id, TEST_ELECTION);
        assert!(fs::metadata(&path).await.is_err());
        assert!(
            fs::metadata(key_path(&dir, stream_id, TEST_ELECTION))
                .await
                .is_err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn streams_by_scope_lists_political_group_streams_only() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let cipher = test_cipher();
        let store = local_store(&dir, stream_id, &cipher);
        store
            .update(TestEvent {
                label: "one".to_string(),
            })
            .await?;

        let political = streams_by_scope(&dir, Scope::PoliticalGroup).await;
        assert_eq!(political, vec![(stream_id, TEST_ELECTION)]);

        let committee = streams_by_scope(&dir, Scope::CentralElectoralCommittee).await;
        assert!(committee.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn find_event_by_hash_prefix_locates_and_detects_ambiguity() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let cipher = test_cipher();
        let store = local_store(&dir, stream_id, &cipher);
        store
            .update(TestEvent {
                label: "one".to_string(),
            })
            .await?;

        let target_hash = store.data.read().last_event_hash();

        // Full hash resolves to the single event.
        assert_eq!(
            find_event_by_hash_prefix(&dir, &target_hash).await?,
            Some((stream_id, TEST_ELECTION, 1))
        );
        // A short prefix still resolves.
        assert_eq!(
            find_event_by_hash_prefix(&dir, &target_hash[..8]).await?,
            Some((stream_id, TEST_ELECTION, 1))
        );
        // A non-matching prefix resolves to nothing.
        assert_eq!(find_event_by_hash_prefix(&dir, &[0xFFu8; 32]).await?, None);
        // The empty prefix matches every event; with one event that is unique.
        assert_eq!(
            find_event_by_hash_prefix(&dir, &[]).await?,
            Some((stream_id, TEST_ELECTION, 1))
        );

        // A second event makes the empty prefix ambiguous.
        store
            .update(TestEvent {
                label: "two".to_string(),
            })
            .await?;
        assert!(matches!(
            find_event_by_hash_prefix(&dir, &[]).await,
            Err(AppError::AmbiguousHash)
        ));

        Ok(())
    }

    #[tokio::test]
    async fn update_uses_last_event_id_from_file() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let cipher = test_cipher();
        let store = local_store(&dir, StreamId::new(), &cipher);
        let path = stream_path(&dir, store.stream_id, TEST_ELECTION);
        append_event(
            &path,
            &cipher,
            5,
            Utc::now(),
            &TestEvent {
                label: "existing".to_string(),
            },
            &GENESIS_HASH,
        )
        .await?;
        store
            .update(TestEvent {
                label: "next".to_string(),
            })
            .await?;

        // Replay and check that the new event got ID 6.
        let fresh = test_store(store.stream_id);
        replay_from_file(&fresh, &dir, &cipher).await?;

        let data = fresh.data.read();
        assert_eq!(data.last_event_id(), 6);
        assert_eq!(data.events.len(), 2);
        assert_eq!(data.events[1].event_id, 6);

        Ok(())
    }

    #[tokio::test]
    async fn different_key_cannot_read_events() -> Result<(), AppError> {
        let dir = temp_dir().await;
        init_local(&dir).await?;

        let stream_id = StreamId::new();
        let cipher = test_cipher();
        let store = local_store(&dir, stream_id, &cipher);
        store
            .update(TestEvent {
                label: "secret".to_string(),
            })
            .await?;

        // Replay with a different stream's key.
        let wrong_cipher = StreamKey::generate().cipher();
        let wrong_store = test_store(stream_id);

        let err = replay_from_file(&wrong_store, &dir, &wrong_cipher)
            .await
            .expect_err("replay must fail with the wrong key");
        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert!(wrong_store.data.read().events.is_empty());

        Ok(())
    }
}
