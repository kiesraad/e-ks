//! Fuzz harnesses for the cargo-fuzz targets in `fuzz/`, built only under
//! `--cfg fuzzing` (cargo-fuzz sets it; see `bin/fuzz`).
//!
//! Each harness takes raw fuzzer bytes, ignores input the code under test
//! rejects, and panics only when an invariant breaks. The harnesses live next
//! to the code they test, so they can reach private items; this module only
//! re-exports them.

pub use crate::{
    auth::csb_webauthn::fuzz::{valid_assertion_seed, webauthn_assertion},
    candidate_lists::importer::fuzz::candidate_csv,
    store::fuzz::store_event_decode,
};

#[cfg(feature = "tls")]
pub use crate::core::proxy_protocol_fuzz::proxy_protocol_v2;

/// Seeds for the corpus (`fuzz/seeds/<target>/<name>`) and harness self-checks,
/// run by `bin/fuzz seeds`.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{
        CsbAction, CsbMainAction, CsbUser, PgEvent,
        store::fuzz::encode_seed,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{sample_candidate_list, sample_person, sample_political_group},
    };

    const PROXY_SIGNATURE: &[u8] = b"\r\n\r\n\0\r\nQUIT\n";

    fn proxy_seed(family: u8, addresses: &[u8]) -> Vec<u8> {
        // Even marker byte: the signature is part of the input.
        let mut seed = vec![0];
        seed.extend_from_slice(PROXY_SIGNATURE);
        seed.extend_from_slice(&[0x21, family]);
        seed.extend_from_slice(&u16::try_from(addresses.len()).expect("short").to_be_bytes());
        seed.extend_from_slice(addresses);
        seed
    }

    /// `(target, file name, bytes)` for every generated seed.
    fn seeds() -> Vec<(&'static str, &'static str, Vec<u8>)> {
        let ipv4 = [[203, 0, 113, 7], [172, 16, 12, 1]].concat();
        let ipv6 = [[0x20; 16], [0xfd; 16]].concat();
        let ports = [0xC8, 0x22, 0x20, 0xFB];
        let user = CsbUser::new_test();
        vec![
            (
                "proxy_protocol_v2",
                "ipv4",
                proxy_seed(0x11, &[&ipv4[..], &ports].concat()),
            ),
            (
                "proxy_protocol_v2",
                "ipv6",
                proxy_seed(0x21, &[&ipv6[..], &ports].concat()),
            ),
            ("webauthn_assertion", "valid", valid_assertion_seed()),
            ("webauthn_assertion", "variation", vec![1; 65]),
            (
                "store_event_decode",
                "create_person",
                encode_seed(
                    0,
                    &PgEvent::CreatePerson(Box::new(sample_person(PersonId::new()))),
                ),
            ),
            (
                "store_event_decode",
                "create_candidate_list",
                encode_seed(
                    0,
                    &PgEvent::CreateCandidateList(Box::new(sample_candidate_list(
                        CandidateListId::new(),
                    ))),
                ),
            ),
            (
                "store_event_decode",
                "update_political_group",
                encode_seed(0, &PgEvent::UpdatePoliticalGroup(sample_political_group())),
            ),
            (
                "store_event_decode",
                "csb_set_finished",
                encode_seed(1, &CsbAction::SetFinished(true).by(user.clone())),
            ),
            (
                "store_event_decode",
                "csb_main_logout",
                encode_seed(2, &CsbMainAction::Logout.by(user)),
            ),
        ]
    }

    fn seed_dir(target: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fuzz/seeds")
            .join(target)
    }

    #[test]
    fn write_seeds() {
        for (target, name, bytes) in seeds() {
            let dir = seed_dir(target);
            std::fs::create_dir_all(&dir).expect("seed dir");
            std::fs::write(dir.join(name), bytes).expect("seed written");
        }
    }

    /// Every seed passes its harness: the oracles hold on valid input.
    #[test]
    fn harnesses_accept_their_seeds() {
        for (target, _, bytes) in seeds() {
            run(target, &bytes);
        }
        for entry in std::fs::read_dir(seed_dir("candidate_csv")).expect("csv seeds") {
            candidate_csv(&std::fs::read(entry.expect("entry").path()).expect("seed"));
        }
    }

    fn run(target: &str, bytes: &[u8]) {
        match target {
            #[cfg(feature = "tls")]
            "proxy_protocol_v2" => proxy_protocol_v2(bytes),
            #[cfg(not(feature = "tls"))]
            "proxy_protocol_v2" => {}
            "webauthn_assertion" => webauthn_assertion(bytes),
            "store_event_decode" => store_event_decode(bytes),
            other => panic!("unknown target {other}"),
        }
    }
}
