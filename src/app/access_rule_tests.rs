//! Access-rule fuzzer: random request sequences from several actors against
//! the real router, checking the access invariants after every step.
//!
//! Actors: political groups A and B (same election) and C (another election),
//! a committee member, one correcting the paper documents of A's import, and
//! clients without a (valid) session. Every group's data carries unique
//! markers, so a leak shows up in any response body, header or ZIP entry.
//!
//! Invariants: no 5xx; no foreign marker in a response; writes only land in
//! the requester's own streams; no 2xx outside the requester's area; no change
//! at all from a POST without its session's CSRF token.
//!
//! Needs `RUSTFLAGS="--cfg fuzzing"`; `PROPTEST_CASES` sets the case count.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
    sync::Mutex,
    time::Duration,
};

use async_zip::base::read1::seek::ZipArchiveReader;
use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode, header},
};
use futures_lite::{AsyncReadExt, io::Cursor};
use http_body_util::BodyExt;
use proptest::{
    prelude::*,
    test_runner::{TestCaseError, TestError, TestRunner},
};
use serde::{Serialize, de::DeserializeOwned};
use tower::ServiceExt;

use crate::{
    AppState, CsbAction, CsbStoreData, ElectionConfig, EventHashPrefix, Locale, PgStore,
    PgStoreData, Province, SESSION_COOKIE_NAME, Scope, Session, SessionUser, StreamId,
    brp_stub::BrpStub,
    csb::import::brp_sweep_running,
    projection::CSB_MAIN_STREAM_ID,
    store::{StoreData, StoreRegistry},
    structs::{
        brp::BrpClient, candidate_lists::CandidateListId, common::PreviousElectionResults,
        list_designation::ListDesignation, list_submitters::ListSubmitterId,
        name_authorisations::NameAuthorisationId, persons::PersonId,
        political_groups::PoliticalGroup,
    },
    test_utils::{
        proptest_config, sample_candidate_list, sample_full_name, sample_list_submitter,
        sample_name_authorisation, sample_person_with,
    },
};

/// Cases per run unless `PROPTEST_CASES` says otherwise.
const DEFAULT_CASES: u32 = 768;

/// The committee's election (the test config's default election).
const CSB_ELECTION: ElectionConfig = ElectionConfig::EK27;

/// A cookie no session was ever issued for.
const FORGED_TOKEN: &str = "Forged0000000000000000000000000000000000x";

/// Path values that are no valid id, or name nothing.
const JUNK: &[&str] = &[
    "x",
    "0",
    "-1",
    "..",
    "%2e%2e",
    "a%20b",
    "00000000-0000-0000-0000-000000000000",
    "0000000000000000",
    "ffffffffffffffff",
    "99999999999999999999",
    "%00",
    "nl",
];

/// Bodies no form expects.
const JUNK_BODIES: &[&str] = &[
    "a=b",
    "last_name=&initials=",
    "%%%=%",
    "{\"person_ids\":[\"x\"]}",
    "csrf_token=",
    "last_name=%3Cscript%3E&initials=A.&election=XX27&hash=zz",
];

/// Extra query strings.
const QUERIES: &[&str] = &["", "page=999", "sort=x&order=y", "success=1", "list=x"];

// ---------------------------------------------------------------------------
// Route table

/// Who a route is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    /// No session needed.
    Public,
    /// Any session.
    Session,
    /// Political-group store routes; committee only in paper-corrections mode.
    Pg,
    /// Political-group routes closed to paper corrections.
    PgOnly,
    /// Committee routes.
    Csb,
}

struct Route {
    path: &'static str,
    area: Area,
    get: bool,
    post: bool,
}

const fn get(path: &'static str, area: Area) -> Route {
    Route {
        path,
        area,
        get: true,
        post: false,
    }
}

const fn post(path: &'static str, area: Area) -> Route {
    Route {
        path,
        area,
        get: false,
        post: true,
    }
}

const fn both(path: &'static str, area: Area) -> Route {
    Route {
        path,
        area,
        get: true,
        post: true,
    }
}

use Area::{Csb, Pg, PgOnly, Public, Session as Ses};

/// The route table of `crate::router::create`, minus `/dev/login` (a dev-only
/// identity switch) and `/static` (proxied to the asset dev server).
const ROUTES: &[Route] = &[
    both("/login", Public),
    get("/logged-out", Public),
    get("/robots.txt", Public),
    get("/.well-known/security.txt", Public),
    get("/health", Public),
    get("/lb-health", Public),
    get("/saml/sp/metadata", Public),
    get("/saml/sp/acs", Public),
    post("/saml/sp/logout", Public),
    get("/saml/sp/autosubmit.js", Public),
    get("/csb/login", Public),
    post("/csb/login/finish", Public),
    post("/language", Ses),
    both("/select-election", Ses),
    both("/session", Ses),
    both("/logout", Ses),
    get("/lookup?pc=1234AB&n=10", Ses),
    get("/suggest?wp=Juinen", Ses),
    get("/", Pg),
    both("/switch-election", Pg),
    post("/hide-download-warning", Pg),
    get("/audit-log", Pg),
    get("/audit-log/{event_id}", Pg),
    get(
        "/audit-log/{event_id}/{event_hash}/{locale}/documents.zip",
        PgOnly,
    ),
    get("/persons", Pg),
    both("/persons/create", Pg),
    both("/persons/{person_id}/update", Pg),
    both("/persons/{person_id}/delete", Pg),
    both("/persons/{person_id}/address", Pg),
    both("/persons/{person_id}/representative", Pg),
    get("/candidate-lists", Pg),
    both("/candidate-lists/create", Pg),
    get("/candidate-lists/{list_id}", Pg),
    both("/candidate-lists/{list_id}/update", Pg),
    both("/candidate-lists/{list_id}/delete", Pg),
    post("/candidate-lists/{list_id}/reorder", Pg),
    get("/candidate-lists/{list_id}/export/{event_hash}", Pg),
    get("/candidate-lists/export-template", Pg),
    both("/candidate-lists/{list_id}/import", Pg),
    both("/candidate-lists/{list_id}/reorder/{person_id}", Pg),
    both("/candidate-lists/{list_id}/representative/{person_id}", Pg),
    both("/candidate-lists/{list_id}/update/{person_id}", Pg),
    both("/candidate-lists/{list_id}/address/{person_id}", Pg),
    both("/candidate-lists/{list_id}/delete/{person_id}", Pg),
    both("/candidate-lists/{list_id}/add", Pg),
    both("/candidate-lists/{list_id}/create", Pg),
    both("/political-group", Pg),
    both("/political-group/information", Pg),
    get("/political-group/name-authorisation", Pg),
    both("/political-group/name-authorisation/create", Pg),
    both(
        "/political-group/name-authorisation/{authorisation_id}/update",
        Pg,
    ),
    both(
        "/political-group/name-authorisation/{authorisation_id}/delete",
        Pg,
    ),
    get("/political-group/list-submitter", Pg),
    both("/political-group/list-submitter/update", Pg),
    both("/political-group/substitute-submitters/create", Pg),
    both(
        "/political-group/substitute-submitters/{sub_submitter_id}/update",
        Pg,
    ),
    both(
        "/political-group/substitute-submitters/{sub_submitter_id}/delete",
        Pg,
    ),
    get("/does-not-exist", Pg),
    get("/finalise", PgOnly),
    get("/generate/{event_hash}/{locale}/documents.zip", PgOnly),
    get("/csb", Csb),
    get("/csb/eml110a.eml.xml", Csb),
    get("/csb/audit-log", Csb),
    get("/csb/audit-log/{stream_id}/{event_id}", Csb),
    get("/csb/monitoring", Csb),
    both("/csb/import", Csb),
    post("/csb/create-empty", Csb),
    get("/csb/examination", Csb),
    get("/csb/examination/i1.pdf", Csb),
    get("/csb/examination/i1.docx", Csb),
    get("/csb/examination/i4.pdf", Csb),
    get("/csb/examination/i4.docx", Csb),
    get("/csb/examination/{stream_id}", Csb),
    get("/csb/examination/{stream_id}/verzuimbrief.pdf", Csb),
    get("/csb/examination/{stream_id}/verzuimbrief.docx", Csb),
    post("/csb/examination/{stream_id}/toggle-finish", Csb),
    get("/csb/examination/{stream_id}/brp-errors", Csb),
    post("/csb/examination/{stream_id}/brp-check", Csb),
    get("/csb/examination/{stream_id}/brp-check/status", Csb),
    get("/csb/examination/{stream_id}/brp-overzicht.pdf", Csb),
    get("/csb/examination/{stream_id}/brp-overzicht.docx", Csb),
    get("/csb/examination/{stream_id}/brp-overzicht.md", Csb),
    both("/csb/examination/{stream_id}/delete", Csb),
    get("/csb/examination/{stream_id}/general-information", Csb),
    get("/csb/examination/finish", Csb),
    get("/csb/examination/finish/{stream_id}", Csb),
    get("/csb/examination/finish/verzuimbrieven.zip", Csb),
    both("/csb/examination/hearing-details/{model}", Csb),
    post("/csb/examination/{stream_id}/paper-corrections", Csb),
    post("/csb/examination/{stream_id}/paper-corrections/stop", Csb),
    get("/csb/examination/{stream_id}/list/{list_id}", Csb),
    get(
        "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}",
        Csb,
    ),
    post(
        "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}/brp-check",
        Csb,
    ),
    post(
        "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}/brp-finding/{index}/handled",
        Csb,
    ),
    both(
        "/csb/examination/{stream_id}/omission/{omission_type}/{reference}",
        Csb,
    ),
    get(
        "/csb/examination/{stream_id}/omission/{omission_type}/{reference}/overview",
        Csb,
    ),
    post(
        "/csb/examination/{stream_id}/delete-omission/{omission_id}",
        Csb,
    ),
    get("/csb/examination/{stream_id}/omissions", Csb),
    both("/csb/examination/{stream_id}/correction/appellation", Csb),
    both(
        "/csb/examination/{stream_id}/correction/person/{person_id}/{field}",
        Csb,
    ),
    get("/csb/examination/publicatie-kandidatenlijsten.pdf", Csb),
    get("/csb/examination/publicatie-kandidatenlijsten.docx", Csb),
    get("/csb/examination/overzicht-kandidatenlijsten.pdf", Csb),
    get("/csb/examination/overzicht-kandidatenlijsten.docx", Csb),
    get("/csb/finalise", Csb),
    post("/csb/finalise/order", Csb),
    both("/csb/finalise/objection/add", Csb),
    both("/csb/finalise/objection/update/{id}", Csb),
    post("/csb/finalise/objection/delete/{id}", Csb),
    get("/csb/finalise/eml.zip", Csb),
    get("/csb/finalise/i4.pdf", Csb),
    get("/csb/finalise/i4.docx", Csb),
    get("/csb/finalise/overzicht-kandidatenlijsten.zip", Csb),
    get("/csb/pre-submission", Csb),
    both("/csb/pre-submission/import", Csb),
    get("/csb/pre-submission/{stream_id}", Csb),
    post("/csb/pre-submission/{stream_id}/brp-check", Csb),
    get("/csb/pre-submission/{stream_id}/brp-check/status", Csb),
    get("/csb/pre-submission/{stream_id}/brp-overzicht.pdf", Csb),
    get("/csb/pre-submission/{stream_id}/brp-overzicht.docx", Csb),
    get("/csb/pre-submission/{stream_id}/brp-overzicht.md", Csb),
    get("/csb/recovery", Csb),
    get("/csb/recovery/{stream_id}", Csb),
    get("/csb/recovery/{stream_id}/general-information", Csb),
    get("/csb/recovery/{stream_id}/list/{list_id}", Csb),
    get(
        "/csb/recovery/{stream_id}/list/{list_id}/candidate/{person_id}",
        Csb,
    ),
    get("/csb/recovery/{stream_id}/omissions", Csb),
    post(
        "/csb/recovery/{stream_id}/omission/{omission_id}/status",
        Csb,
    ),
    get("/csb/registered-political-groups", Csb),
    both("/csb/registered-political-groups/add", Csb),
    both("/csb/registered-political-groups/{id}", Csb),
    both("/csb/registered-political-groups/{id}/delete", Csb),
    get("/csb/{*path}", Csb),
];

// ---------------------------------------------------------------------------
// Action model

/// Whose session cookie a request carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Who {
    A,
    B,
    C,
    Csb,
    /// A committee member correcting the paper documents of A's import.
    Paper,
    /// No cookie.
    Anon,
    /// A cookie no session was issued for.
    Forged,
}

impl Who {
    /// Slot in the session tokens (A, B, C, committee, paper).
    fn slot(self) -> Option<usize> {
        match self {
            Who::A => Some(0),
            Who::B => Some(1),
            Who::C => Some(2),
            Who::Csb => Some(3),
            Who::Paper => Some(4),
            Who::Anon | Who::Forged => None,
        }
    }

    /// Index of a political group.
    fn group(self) -> Option<usize> {
        self.slot().filter(|slot| *slot < 3)
    }

    /// The areas a well-behaved client of this kind browses.
    fn areas(self) -> &'static [Area] {
        match self {
            Who::A | Who::B | Who::C => &[Pg, PgOnly, Ses],
            Who::Csb => &[Csb, Ses, Pg],
            Who::Paper => &[Pg, Ses, Csb],
            Who::Anon | Who::Forged => &[Public],
        }
    }
}

/// How a path parameter is filled in, resolved against the state at the time
/// the step runs.
#[derive(Clone, Copy, Debug)]
enum Pick {
    /// The n-th id of the requester's own data.
    Own(u8),
    /// The n-th id of another actor's data.
    Of(Who, u8),
    /// A well-formed id nobody has.
    Fresh,
    /// A malformed value from [`JUNK`].
    Junk(u8),
}

/// Which CSRF token a POST presents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Csrf {
    /// The token of the session the cookie belongs to.
    Own,
    Missing,
    /// The token of another actor's session.
    Of(Who),
    Garbage,
}

/// How a POST carries its token and body.
#[derive(Clone, Copy, Debug)]
enum Transport {
    /// Urlencoded body with a `csrf_token` field.
    Form,
    /// Urlencoded body, token in the `x-csrf-token` header.
    Header,
    /// Multipart body, token in the query string.
    Multipart,
    /// JSON body, token in the header.
    Json,
}

#[derive(Clone, Copy, Debug)]
enum Payload {
    /// Fields every form of the app accepts; `n` picks variants.
    Plausible(u8),
    /// A body from [`JUNK_BODIES`].
    Junk(u8),
    Empty,
}

/// The `sec-fetch-site` header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fetch {
    Absent,
    SameOrigin,
    CrossSite,
}

/// One request.
#[derive(Clone)]
struct Step {
    who: Who,
    /// Steer around known findings (see `name_a_candidate`); off in replays.
    narrow: bool,
    /// Pick the route among the ones meant for `who`.
    aligned: bool,
    route: u16,
    post: bool,
    params: [Pick; 4],
    query: u8,
    csrf: Csrf,
    transport: Transport,
    payload: Payload,
    fetch: Fetch,
}

impl Step {
    fn route(&self) -> &'static Route {
        let routes: Vec<&'static Route> = ROUTES
            .iter()
            .filter(|route| !self.aligned || self.who.areas().contains(&route.area))
            .collect();
        routes[usize::from(self.route) % routes.len()]
    }

    fn is_post(&self) -> bool {
        let route = self.route();
        if self.post { route.post } else { !route.get }
    }

    /// A same-origin request with the session's own token, for replays.
    fn replay(who: Who, post: bool, path: &str, params: &[Pick], payload: Payload) -> Self {
        let route = ROUTES
            .iter()
            .position(|route| route.path == path)
            .expect("route");
        let mut step = Step {
            who,
            narrow: false,
            aligned: false,
            route: u16::try_from(route).expect("route index"),
            post,
            params: [Pick::Fresh; 4],
            query: 0,
            csrf: Csrf::Own,
            transport: Transport::Form,
            payload,
            fetch: Fetch::Absent,
        };
        step.params[..params.len()].copy_from_slice(params);
        step
    }
}

/// One line per step, so a shrunk sequence reads as a request log.
impl fmt::Debug for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let route = self.route();
        let verb = if self.is_post() { "POST" } else { "GET" };
        write!(f, "{:?} {verb} {}", self.who, route.path)?;
        let placeholders = route.path.matches('{').count();
        if placeholders > 0 {
            write!(f, " {:?}", &self.params[..placeholders])?;
        }
        if self.query > 0 {
            write!(f, " ?{}", QUERIES[usize::from(self.query) % QUERIES.len()])?;
        }
        if self.is_post() {
            write!(
                f,
                " csrf={:?} via={:?} body={:?}",
                self.csrf, self.transport, self.payload
            )?;
        }
        if self.fetch != Fetch::Absent {
            write!(f, " fetch={:?}", self.fetch)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Case {
    /// Seeds the markers.
    seed: u64,
    steps: Vec<Step>,
}

fn who() -> impl Strategy<Value = Who> {
    prop_oneof![
        4 => Just(Who::A),
        3 => Just(Who::B),
        2 => Just(Who::C),
        3 => Just(Who::Csb),
        2 => Just(Who::Paper),
        1 => Just(Who::Anon),
        1 => Just(Who::Forged),
    ]
}

fn known() -> impl Strategy<Value = Who> {
    prop_oneof![
        Just(Who::A),
        Just(Who::B),
        Just(Who::C),
        Just(Who::Csb),
        Just(Who::Paper)
    ]
}

fn pick() -> impl Strategy<Value = Pick> {
    prop_oneof![
        6 => (0..4u8).prop_map(Pick::Own),
        3 => (known(), 0..4u8).prop_map(|(who, n)| Pick::Of(who, n)),
        1 => Just(Pick::Fresh),
        1 => (0..JUNK.len() as u8).prop_map(Pick::Junk),
    ]
}

fn csrf() -> impl Strategy<Value = Csrf> {
    prop_oneof![
        8 => Just(Csrf::Own),
        1 => Just(Csrf::Missing),
        1 => known().prop_map(Csrf::Of),
        1 => Just(Csrf::Garbage),
    ]
}

fn transport() -> impl Strategy<Value = Transport> {
    prop_oneof![
        6 => Just(Transport::Form),
        1 => Just(Transport::Header),
        1 => Just(Transport::Multipart),
        1 => Just(Transport::Json),
    ]
}

fn payload() -> impl Strategy<Value = Payload> {
    prop_oneof![
        8 => (0..6u8).prop_map(Payload::Plausible),
        1 => (0..JUNK_BODIES.len() as u8).prop_map(Payload::Junk),
        1 => Just(Payload::Empty),
    ]
}

fn fetch() -> impl Strategy<Value = Fetch> {
    prop_oneof![
        8 => Just(Fetch::Absent),
        1 => Just(Fetch::SameOrigin),
        1 => Just(Fetch::CrossSite),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    (
        (
            who(),
            prop::bool::weighted(0.8),
            any::<u16>(),
            any::<bool>(),
        ),
        prop::array::uniform4(pick()),
        prop_oneof![6 => Just(0u8), 1 => 1..QUERIES.len() as u8],
        (csrf(), transport(), payload(), fetch()),
    )
        .prop_map(
            |((who, aligned, route, post), params, query, (csrf, transport, payload, fetch))| {
                let mut step = Step {
                    who,
                    narrow: true,
                    aligned,
                    route,
                    post,
                    params,
                    query,
                    csrf,
                    transport,
                    payload,
                    fetch,
                };
                // Finding, see `finding_cleared_appellation_breaks_the_candidate_list_overview`:
                // keep the appellation set.
                if step.is_post() && step.route().path == "/political-group/information" {
                    step.payload = Payload::Plausible(0);
                }
                step
            },
        )
}

fn case() -> impl Strategy<Value = Case> {
    (any::<u64>(), prop::collection::vec(step(), 1..40))
        .prop_map(|(seed, steps)| Case { seed, steps })
}

// ---------------------------------------------------------------------------
// World

/// A political group with its seeded stream and markers.
struct PgActor {
    stream: StreamId,
    election: ElectionConfig,
    /// In persons, submitters and the name authorisation.
    secret: String,
    /// In the appellation, which the committee's monitoring page shows for
    /// every warm stream by design.
    appellation: String,
}

/// A marker no page contains by chance: `Zq`, a tag letter, 8 letters.
fn marker(seed: u64, tag: u8) -> String {
    let mut state = seed ^ u64::from(tag).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut marker = format!("Zq{}", char::from(tag));
    for _ in 0..8 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        marker.push(char::from(
            b'a' + u8::try_from((state >> 33) % 26).expect("letter"),
        ));
    }
    marker
}

struct World {
    state: AppState,
    app: Router,
    pg: Vec<PgActor>,
    /// Session tokens, by [`Who::slot`].
    tokens: Vec<String>,
}

/// The session (if any) a request runs under.
#[derive(Clone, Debug)]
enum Identity {
    Anonymous,
    Pg {
        actor: Option<usize>,
        stream: StreamId,
        election: Option<ElectionConfig>,
    },
    Csb {
        paper: Option<StreamId>,
    },
}

struct Viewer {
    identity: Identity,
    cookie: Option<String>,
    csrf: Option<String>,
}

impl World {
    async fn new(seed: u64, brp: &BrpClient) -> Self {
        let mut state = AppState::new_for_tests().await;
        state.brp_client = brp.clone();

        let elections = [
            ElectionConfig::EK27,
            ElectionConfig::EK27,
            ElectionConfig::PS27(Province::Groningen),
        ];
        let mut pg = Vec::new();
        let mut tokens = Vec::new();
        for (tag, election) in (0u8..).zip(elections) {
            let actor = PgActor {
                stream: StreamId::new(),
                election,
                secret: marker(seed, b'a' + tag),
                appellation: marker(seed, b'k' + tag),
            };
            seed_pg(&state, &actor).await;
            let session = Session::for_political_group(
                actor.stream,
                String::new(),
                Some(election),
                Locale::default(),
            );
            tokens.push(insert_session(&state, session).await);
            pg.push(actor);
        }
        let imported = import_into_csb(&state, &pg[0]).await;
        tokens.push(insert_session(&state, Session::new_test_committee()).await);
        let mut paper = Session::new_test_committee();
        paper
            .set_paper_correction_stream_id(Some(imported))
            .expect("committee session");
        tokens.push(insert_session(&state, paper).await);

        let app = crate::router::create(state.clone()).with_state(state.clone());
        Self {
            state,
            app,
            pg,
            tokens,
        }
    }

    async fn session(&self, token: Option<&str>) -> Option<Session> {
        self.state
            .sessions
            .get_existing(token)
            .await
            .expect("session lookup")
    }

    async fn viewer(&self, who: Who) -> Viewer {
        let cookie = match who {
            Who::Anon => None,
            Who::Forged => Some(FORGED_TOKEN.to_string()),
            known => Some(self.tokens[known.slot().expect("slot")].clone()),
        };
        let session = self.session(cookie.as_deref()).await;
        let identity = match session.as_ref().map(|s| &s.user) {
            None => Identity::Anonymous,
            Some(SessionUser::PoliticalGroup {
                stream_id,
                election,
                ..
            }) => Identity::Pg {
                actor: self.pg.iter().position(|a| a.stream == *stream_id),
                stream: *stream_id,
                election: *election,
            },
            Some(SessionUser::CentralElectoralCommittee {
                paper_correction_stream_id,
                ..
            }) => Identity::Csb {
                paper: *paper_correction_stream_id,
            },
        };
        Viewer {
            identity,
            cookie,
            csrf: session.map(|s| s.csrf_token().0.clone()),
        }
    }

    /// Examination and pre-submission streams, sorted.
    async fn csb_streams(&self) -> Vec<StreamId> {
        let mut streams = Vec::new();
        for registry in [
            &self.state.csb_store_registry,
            &self.state.pre_submission_store_registry,
        ] {
            let listed = registry.streams_by_scope().await.expect("streams");
            streams.extend(listed.into_iter().map(|(stream, _)| stream));
        }
        streams.sort();
        streams
    }

    fn csb_store(&self, stream: StreamId) -> Option<crate::CsbStream> {
        self.state
            .csb_store_registry
            .get_cached(stream, CSB_ELECTION)
            .or_else(|| {
                self.state
                    .pre_submission_store_registry
                    .get_cached(stream, CSB_ELECTION)
            })
    }

    fn pg_ids(&self, stream: StreamId, election: Option<ElectionConfig>) -> Ids {
        election
            .and_then(|election| self.state.store_registry.get_cached(stream, election))
            .map(|store| Ids::from_pg(&store.snapshot()))
            .unwrap_or_default()
    }

    /// The data behind a stream id: a committee stream or a group's stream.
    fn stream_ids(&self, stream: StreamId) -> Ids {
        let mut ids = match (
            self.csb_store(stream),
            self.pg.iter().find(|a| a.stream == stream),
        ) {
            (Some(store), _) => Ids::from_csb(&store.snapshot()),
            (None, Some(actor)) => self.pg_ids(actor.stream, Some(actor.election)),
            (None, None) => Ids::default(),
        };
        ids.stream = Some(stream);
        ids
    }

    fn actor_ids(&self, actor: usize) -> Ids {
        let actor = &self.pg[actor];
        self.pg_ids(actor.stream, Some(actor.election))
    }

    /// Committee main store ids: registered groups and objections.
    fn main_ids(&self) -> (Vec<String>, Vec<String>) {
        let Some(store) = self
            .state
            .csb_main_store_registry
            .get_cached(CSB_MAIN_STREAM_ID, CSB_ELECTION)
        else {
            return (Vec::new(), Vec::new());
        };
        let data = store.snapshot();
        let groups = data.registered_political_groups.iter().map(|g| g.id);
        let objections = data.objections.iter().map(|o| o.id);
        (sorted(groups), sorted(objections))
    }

    /// Wait until no BRP sweep runs, so the next snapshot is stable.
    async fn settle(&self) {
        for _ in 0..5000 {
            let streams = self.csb_streams().await;
            if !streams.into_iter().any(brp_sweep_running) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("a BRP sweep did not finish");
    }
}

async fn insert_session(state: &AppState, session: Session) -> String {
    let token = session.token_string();
    state.sessions.insert(session).await;
    token
}

/// Seed a group's stream with marker-carrying data.
async fn seed_pg(state: &AppState, actor: &PgActor) {
    let store = PgStore::own(
        state
            .store_for_stream(actor.stream, actor.election, false)
            .await
            .expect("store"),
    );
    PoliticalGroup {
        appellation: Some(
            format!("{} Partij", actor.appellation)
                .parse()
                .expect("appellation"),
        ),
        list_designation: Some(ListDesignation::Standalone),
        previous_election_results: Some(PreviousElectionResults::ZeroSeats),
    }
    .update(&store)
    .await
    .expect("group");

    let mut list = sample_candidate_list(CandidateListId::new());
    list.electoral_districts = BTreeSet::from([actor.election.electoral_districts()[0]]);
    for _ in 0..3 {
        let id = PersonId::new();
        sample_person_with(id, Some(&actor.secret), &actor.secret, None, "A.B.")
            .create(&store)
            .await
            .expect("person");
        list.candidates.push(id);
    }
    list.create(&store).await.expect("list");

    let mut agent = sample_name_authorisation(NameAuthorisationId::new());
    agent.legal_name = format!("{} Stichting", actor.secret)
        .parse()
        .expect("legal name");
    agent.create(&store).await.expect("agent");

    for is_substitute in [false, true] {
        let mut submitter = sample_list_submitter(ListSubmitterId::new());
        submitter.name = sample_full_name(None, &actor.secret, None, "E.F.");
        submitter.is_substitute = is_substitute;
        if is_substitute {
            submitter
                .create_substitute(&store)
                .await
                .expect("substitute");
        } else {
            submitter.update(&store).await.expect("submitter");
        }
    }
}

/// Import `actor`'s stream for examination, as the committee does by hash,
/// returning the new committee stream.
async fn import_into_csb(state: &AppState, actor: &PgActor) -> StreamId {
    let source = state
        .store_for_stream(actor.stream, actor.election, false)
        .await
        .expect("source");
    let csb = state
        .csb_store_for_stream(StreamId::new(), actor.election)
        .await
        .expect("csb store")
        .acting_as_test_user();
    let events = source.snapshot().events.clone();
    csb.update(CsbAction::Import {
        hash: source.current_event_hash(),
        source_stream_id: actor.stream,
        snapshot: Box::new(PgStoreData::snapshot_until(
            &events,
            source.current_event_id(),
        )),
    })
    .await
    .expect("import");
    csb.stream_id
}

fn sorted<T: Ord + ToString>(items: impl Iterator<Item = T>) -> Vec<String> {
    let mut items: Vec<T> = items.collect();
    items.sort();
    items.iter().map(ToString::to_string).collect()
}

/// Ids found in one stream's data.
#[derive(Default)]
struct Ids {
    persons: Vec<String>,
    lists: Vec<String>,
    authorisations: Vec<String>,
    submitters: Vec<String>,
    hashes: Vec<[u8; 32]>,
    omissions: Vec<String>,
    stream: Option<StreamId>,
    candidates: HashMap<String, Vec<String>>,
}

impl Ids {
    fn from_pg(data: &PgStoreData) -> Self {
        let mut ids = Self::default();
        ids.add_pg(data);
        ids.hashes = data.events.iter().map(|e| e.hash).collect();
        ids
    }

    fn add_pg(&mut self, data: &PgStoreData) {
        self.persons.extend(sorted(data.persons.keys()));
        self.lists.extend(sorted(data.candidate_lists.keys()));
        self.authorisations
            .extend(sorted(data.name_authorisations.keys()));
        self.submitters
            .extend(sorted(data.substitute_submitters.iter().map(|s| s.id)));
        for list in data.candidate_lists.values() {
            self.candidates.insert(
                list.id.to_string(),
                list.candidates.iter().map(ToString::to_string).collect(),
            );
        }
    }

    fn from_csb(data: &CsbStoreData) -> Self {
        let mut ids = Self::default();
        ids.add_pg(&data.imported_data);
        ids.add_pg(&data.paper_corrected_data);
        ids.hashes = data.events.iter().map(|e| e.hash).collect();
        ids.omissions = sorted(data.omissions.keys());
        ids
    }
}

fn nth(values: &[String], n: u8) -> Option<String> {
    (!values.is_empty()).then(|| values[usize::from(n) % values.len()].clone())
}

/// A value of the right shape that names nothing.
fn fresh(name: &str) -> String {
    let uuid = uuid::Uuid::new_v4();
    match name {
        "event_id" | "index" => "999999".to_string(),
        "event_hash" => uuid.simple().to_string()[..16].to_string(),
        "locale" => "nl".to_string(),
        "model" => "i1".to_string(),
        "omission_type" => "candidate".to_string(),
        "field" => "last-name".to_string(),
        "*path" => "does-not-exist".to_string(),
        _ => uuid.to_string(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Resolving a step into a request

/// What a step resolved to.
struct Resolved {
    path: String,
    values: HashMap<String, String>,
    /// The data the requester addresses (its own, or the committee stream in
    /// the path).
    own: Ids,
}

impl World {
    /// The data a request addresses as "own": the committee stream in a
    /// committee path, else the session's store.
    fn own_ids(&self, viewer: &Viewer, area: Area, stream: Option<StreamId>) -> Ids {
        match (&viewer.identity, area, stream) {
            (_, Csb, Some(stream)) => self.stream_ids(stream),
            (
                Identity::Pg {
                    stream, election, ..
                },
                _,
                _,
            ) => self.pg_ids(*stream, *election),
            (Identity::Csb { paper: Some(paper) }, _, _) => self.stream_ids(*paper),
            _ => Ids::default(),
        }
    }

    async fn resolve(&self, step: &Step, viewer: &Viewer) -> Resolved {
        let route = step.route();
        let csb_streams = self.csb_streams().await;
        let names: Vec<&str> = route.path.split(['{', '}']).skip(1).step_by(2).collect();

        let mut values = HashMap::new();
        let mut stream = None;
        if names.first() == Some(&"stream_id") {
            let value = self.stream_value(step.params[0], &csb_streams);
            stream = value.parse().ok();
            values.insert("stream_id".to_string(), value);
        }
        let own = self.own_ids(viewer, route.area, stream);
        let committee = csb_streams
            .first()
            .map(|stream| self.stream_ids(*stream))
            .unwrap_or_default();
        for (name, pick) in names.iter().zip(step.params) {
            if !values.contains_key(*name) {
                let ids = match pick {
                    Pick::Of(who, _) if who.group().is_none() => &committee,
                    _ => &own,
                };
                let value = self.value(name, pick, ids, route.path);
                values.insert((*name).to_string(), value);
            }
        }
        name_a_candidate(step, &own, &mut values);

        let mut path = route.path.to_string();
        for (name, value) in &values {
            path = path.replace(&format!("{{{name}}}"), value);
        }
        Resolved { path, values, own }
    }

    fn stream_value(&self, pick: Pick, csb_streams: &[StreamId]) -> String {
        let streams = sorted(csb_streams.iter());
        match pick {
            Pick::Own(n) => nth(&streams, n).unwrap_or_else(|| fresh("stream_id")),
            Pick::Of(who, _) => match who.group() {
                Some(group) => self.pg[group].stream.to_string(),
                None => CSB_MAIN_STREAM_ID.to_string(),
            },
            Pick::Fresh => fresh("stream_id"),
            Pick::Junk(n) => JUNK[usize::from(n) % JUNK.len()].to_string(),
        }
    }

    /// `ids` is the requester's own data, or the committee's for a pick of a
    /// committee member's.
    fn value(&self, name: &str, pick: Pick, ids: &Ids, path: &str) -> String {
        let value = match pick {
            Pick::Own(n) => self.value_in(name, n, ids, path),
            Pick::Of(who, n) => match who.group() {
                Some(group) => self.value_in(name, n, &self.actor_ids(group), path),
                None => self.value_in(name, n, ids, path),
            },
            Pick::Fresh => None,
            Pick::Junk(n) => Some(JUNK[usize::from(n) % JUNK.len()].to_string()),
        };
        value.unwrap_or_else(|| fresh(name))
    }

    fn value_in(&self, name: &str, n: u8, ids: &Ids, path: &str) -> Option<String> {
        let index = usize::from(n);
        match name {
            "person_id" => nth(&ids.persons, n),
            "list_id" => nth(&ids.lists, n),
            "authorisation_id" => nth(&ids.authorisations, n),
            "sub_submitter_id" => nth(&ids.submitters, n),
            "omission_id" => nth(&ids.omissions, n),
            "event_id" | "index" => Some((index + 1).to_string()),
            "event_hash" => (!ids.hashes.is_empty()).then(|| {
                let hash = &ids.hashes[ids.hashes.len() - 1 - index % ids.hashes.len()];
                EventHashPrefix::of(hash).to_string()
            }),
            "locale" => Some(["nl", "fry"][index % 2].to_string()),
            "model" => Some(["i1", "i4"][index % 2].to_string()),
            "omission_type" => Some(OMISSION_TYPES[index % OMISSION_TYPES.len()].to_string()),
            "field" => Some(FIELDS[index % FIELDS.len()].to_string()),
            "reference" => {
                let mut references: Vec<String> =
                    ids.stream.iter().map(ToString::to_string).collect();
                references.extend(ids.lists.iter().chain(&ids.persons).cloned());
                nth(&references, n)
            }
            "id" => {
                let (groups, objections) = self.main_ids();
                nth(
                    if path.contains("objection") {
                        &objections
                    } else {
                        &groups
                    },
                    n,
                )
            }
            _ => None,
        }
    }
}

/// Finding, see `finding_omission_for_an_unknown_candidate_breaks_the_omissions_page`:
/// a candidate omission that is added names one of the stream's candidates.
fn name_a_candidate(step: &Step, own: &Ids, values: &mut HashMap<String, String>) {
    let adds = step.is_post()
        && step
            .route()
            .path
            .ends_with("/omission/{omission_type}/{reference}");
    if !step.narrow || !adds || values.get("omission_type").map(String::as_str) != Some("candidate")
    {
        return;
    }
    if values
        .get("reference")
        .is_some_and(|reference| own.persons.contains(reference))
    {
        return;
    }
    match own.persons.first() {
        Some(person) => values.insert("reference".to_string(), person.clone()),
        None => values.insert("omission_type".to_string(), "political-group".to_string()),
    };
}

const OMISSION_TYPES: &[&str] = &[
    "political-group",
    "appellation",
    "candidate-list",
    "declarations-of-support",
    "candidate",
];

const FIELDS: &[&str] = &[
    "initials",
    "last-name-prefix",
    "last-name",
    "date-of-birth",
    "place-of-residence",
];

// ---------------------------------------------------------------------------
// Request bodies

impl World {
    /// The requester's own markers: a well-behaved client only submits its
    /// own data.
    fn own_markers(&self, viewer: &Viewer) -> (String, String) {
        match &viewer.identity {
            Identity::Pg {
                actor: Some(actor), ..
            } => (
                self.pg[*actor].secret.clone(),
                self.pg[*actor].appellation.clone(),
            ),
            _ => ("Neutraal".to_string(), "Neutraal".to_string()),
        }
    }

    /// Fields the political-group forms accept, all at once.
    fn pg_fields(
        &self,
        viewer: &Viewer,
        resolved: &Resolved,
        n: u8,
    ) -> Vec<(&'static str, String)> {
        let (secret, appellation) = self.own_markers(viewer);
        let election = match &viewer.identity {
            Identity::Pg {
                election: Some(election),
                ..
            } => *election,
            _ => CSB_ELECTION,
        };
        let district = election.electoral_districts()[0].serde_name();
        let (code, domain) = [("EK27", ""), ("PS27", "prov1"), ("WS27", "ws2")][usize::from(n) % 3];
        let action = if resolved.path.ends_with("/add") {
            "add-all"
        } else {
            "save"
        };
        [
            ("first_name", secret.as_str()),
            ("last_name", &secret),
            ("last_name_prefix", ""),
            ("initials", "A.B."),
            ("gender", "female"),
            ("date_of_birth", "01-02-1990"),
            ("bsn", "none-confirmed"),
            ("place_of_residence", "Juinen"),
            ("country", "NL"),
            ("locality", "Juinen"),
            ("postal_code", "1234 AB"),
            ("house_number", "10"),
            ("house_number_addition", "A"),
            ("street_name", "Stationsstraat"),
            ("state_or_province", ""),
            ("appellation", &format!("{appellation} Partij")),
            ("previous_election_results", "zero_seats"),
            ("list_designation_type", "standalone"),
            ("legal_name", &format!("{secret} Stichting")),
            ("electoral_districts", &district),
            ("copy_candidates", "true"),
            ("action", action),
            ("added_position", "1"),
            ("position", "1"),
            ("election", code),
            ("domain_province", domain),
            ("domain_water_council", domain),
            ("lang", ["en", "nl"][usize::from(n) % 2]),
        ]
        .into_iter()
        .map(|(name, value)| (name, value.to_string()))
        .collect()
    }

    /// Fields the committee forms accept, all at once. `hash` names a
    /// group's latest event, as handed over for an import.
    fn csb_fields(&self, resolved: &Resolved, n: u8) -> Vec<(&'static str, String)> {
        let actor = &self.pg[usize::from(n) % self.pg.len()];
        let hash = self
            .state
            .store_registry
            .get_cached(actor.stream, actor.election)
            .map(|store| hex(&store.current_event_hash()))
            .unwrap_or_default();
        let district = CSB_ELECTION.electoral_districts()[0].serde_name();
        let mut fields: Vec<(&'static str, String)> = [
            ("title", "Verzuim"),
            ("description", "Omschrijving"),
            ("help_text", ""),
            ("recoverable", "true"),
            ("objection_text", "Bezwaar"),
            ("value", "Jansen"),
            ("status", "recovered"),
            ("handled", "true"),
            ("date_of_hearing", "01-05-2027"),
            ("time_of_hearing", "10:00"),
            ("chair", "Voorzitter"),
            ("signers", "Lid"),
            ("appellation", "Neutrale Partij"),
            ("previous_votes", "100"),
            ("previous_seats", "1"),
        ]
        .into_iter()
        .map(|(name, value)| (name, value.to_string()))
        .collect();
        fields.push(("hash", hash));
        fields.push(("electoral_districts", district));
        if let Some(list) = resolved.own.lists.first() {
            fields.push(("candidate_lists", list.clone()));
        }
        fields
    }

    fn urlencoded_body(&self, step: &Step, viewer: &Viewer, resolved: &Resolved) -> String {
        match step.payload {
            Payload::Plausible(n) => {
                let fields = if step.route().area == Csb {
                    self.csb_fields(resolved, n)
                } else {
                    self.pg_fields(viewer, resolved, n)
                };
                serde_urlencoded::to_string(fields).expect("urlencoded")
            }
            Payload::Junk(n) => JUNK_BODIES[usize::from(n) % JUNK_BODIES.len()].to_string(),
            Payload::Empty => String::new(),
        }
    }

    /// A reorder of the addressed list, or of the committee's groups.
    async fn json_body(&self, step: &Step, resolved: &Resolved) -> String {
        match step.payload {
            Payload::Plausible(_) => {
                let mut persons = resolved
                    .values
                    .get("list_id")
                    .and_then(|list| resolved.own.candidates.get(list))
                    .cloned()
                    .unwrap_or_else(|| resolved.own.persons.clone());
                persons.reverse();
                let streams = self.csb_streams().await;
                serde_json::json!({ "person_ids": persons, "stream_ids": streams }).to_string()
            }
            Payload::Junk(n) => JUNK_BODIES[usize::from(n) % JUNK_BODIES.len()].to_string(),
            Payload::Empty => String::new(),
        }
    }

    /// A candidate list upload.
    fn multipart_body(&self, step: &Step, viewer: &Viewer) -> String {
        let (secret, _) = self.own_markers(viewer);
        match step.payload {
            Payload::Plausible(_) => format!(
                "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file_data\"; \
                 filename=\"lijst.csv\"\r\nContent-Type: text/csv\r\n\r\n\
                 achternaam;voorletters;geboortedatum\r\n{secret};A.B.;01-02-1990\r\n\
                 --{MULTIPART_BOUNDARY}--\r\n"
            ),
            Payload::Junk(n) => JUNK_BODIES[usize::from(n) % JUNK_BODIES.len()].to_string(),
            Payload::Empty => String::new(),
        }
    }
}

const MULTIPART_BOUNDARY: &str = "fuzz-boundary";

// ---------------------------------------------------------------------------
// Requests and responses

struct Prepared {
    request: Request<Body>,
    post: bool,
    /// The CSRF token the request presents.
    presented: Option<String>,
}

fn append_query(uri: &mut String, query: &str) {
    if !query.is_empty() {
        uri.push(if uri.contains('?') { '&' } else { '?' });
        uri.push_str(query);
    }
}

fn base_request(
    method: &str,
    uri: &str,
    step: &Step,
    viewer: &Viewer,
) -> axum::http::request::Builder {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::REFERER, "http://localhost:3000/");
    if let Some(cookie) = &viewer.cookie {
        builder = builder.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}={cookie}"));
    }
    match step.fetch {
        Fetch::Absent => builder,
        Fetch::SameOrigin => builder.header("sec-fetch-site", "same-origin"),
        Fetch::CrossSite => builder.header("sec-fetch-site", "cross-site"),
    }
}

impl World {
    async fn presented(&self, step: &Step, viewer: &Viewer) -> Option<String> {
        match step.csrf {
            Csrf::Own => viewer.csrf.clone(),
            Csrf::Missing => None,
            Csrf::Of(who) => self.viewer(who).await.csrf,
            Csrf::Garbage => Some("garbage-csrf-token".to_string()),
        }
    }

    async fn prepare(&self, step: &Step, viewer: &Viewer) -> Prepared {
        let resolved = self.resolve(step, viewer).await;
        let mut uri = resolved.path.clone();
        append_query(&mut uri, QUERIES[usize::from(step.query) % QUERIES.len()]);

        if !step.is_post() {
            let request = base_request("GET", &uri, step, viewer)
                .body(Body::empty())
                .expect("request");
            return Prepared {
                request,
                post: false,
                presented: None,
            };
        }

        let presented = self.presented(step, viewer).await;
        let request = self
            .post_request(step, viewer, &resolved, uri, presented.as_deref())
            .await;
        Prepared {
            request,
            post: true,
            presented,
        }
    }

    async fn post_request(
        &self,
        step: &Step,
        viewer: &Viewer,
        resolved: &Resolved,
        mut uri: String,
        token: Option<&str>,
    ) -> Request<Body> {
        let (content_type, body, header_token) = match step.transport {
            Transport::Form => {
                let mut body = self.urlencoded_body(step, viewer, resolved);
                if let Some(token) = token {
                    if !body.is_empty() {
                        body.push('&');
                    }
                    body.push_str(&format!("csrf_token={token}"));
                }
                (URLENCODED.to_string(), body, None)
            }
            Transport::Header => (
                URLENCODED.to_string(),
                self.urlencoded_body(step, viewer, resolved),
                token,
            ),
            Transport::Json => (
                "application/json".to_string(),
                self.json_body(step, resolved).await,
                token,
            ),
            Transport::Multipart => {
                if let Some(token) = token {
                    append_query(&mut uri, &format!("csrf_token={token}"));
                }
                (
                    format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
                    self.multipart_body(step, viewer),
                    None,
                )
            }
        };
        let mut builder =
            base_request("POST", &uri, step, viewer).header(header::CONTENT_TYPE, content_type);
        if let Some(token) = header_token {
            builder = builder.header("x-csrf-token", token);
        }
        builder.body(Body::from(body)).expect("request")
    }
}

const URLENCODED: &str = "application/x-www-form-urlencoded";

/// Status plus everything a client could read, lowercased: headers, body and
/// the entries of (nested) ZIP downloads. PDF text streams stay compressed,
/// so a leak only inside a PDF goes unnoticed.
struct Observed {
    status: StatusCode,
    text: String,
}

async fn observe(response: Response<Body>) -> Observed {
    let status = response.status();
    let mut text = String::new();
    for (name, value) in response.headers() {
        text.push_str(name.as_str());
        text.push_str(": ");
        text.push_str(&String::from_utf8_lossy(value.as_bytes()));
        text.push('\n');
    }
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    text.push_str(&String::from_utf8_lossy(&expand(body.to_vec()).await));
    Observed {
        status,
        text: text.to_lowercase(),
    }
}

/// `bytes` followed by the entries of any ZIP inside, depth first.
async fn expand(bytes: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pending = vec![(bytes, 0u8)];
    while let Some((data, depth)) = pending.pop() {
        if depth < 3 && data.starts_with(b"PK\x03\x04") {
            pending.extend(
                unzip(&data)
                    .await
                    .into_iter()
                    .map(|entry| (entry, depth + 1)),
            );
        }
        out.extend_from_slice(&data);
    }
    out
}

async fn unzip(data: &[u8]) -> Vec<Vec<u8>> {
    let Ok(mut zip) = ZipArchiveReader::open(Cursor::new(data.to_vec())).await else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for index in 0..zip.cdrs().len() {
        let mut entry = Vec::new();
        if let Ok(mut file) = zip.file(index).await {
            file.read_to_end(&mut entry).await.ok();
        }
        entries.push(entry);
    }
    entries
}

/// Up to 120 bytes of context around `at`.
fn snippet(text: &str, at: usize) -> &str {
    let start = (at.saturating_sub(120)..=at)
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(at);
    let end = ((at + 120).min(text.len())..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    &text[start..end]
}

// ---------------------------------------------------------------------------
// Invariants

type StreamKey = (Scope, StreamId, ElectionConfig);

/// Every stream's (event count, last event id, last hash), and every known
/// session's identity, locale and CSRF token.
struct Snapshot {
    streams: HashMap<StreamKey, (usize, usize, [u8; 32])>,
    sessions: Vec<Option<String>>,
    /// Per session: a group's stream, or the committee.
    roles: Vec<Option<String>>,
}

async fn record<D>(
    registry: &StoreRegistry<D>,
    out: &mut HashMap<StreamKey, (usize, usize, [u8; 32])>,
) where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
{
    for meta in registry.stream_metadata_by_scope().await.expect("streams") {
        let (id, hash) = registry
            .get_cached(meta.stream_id, meta.election)
            .map(|store| (store.current_event_id(), store.current_event_hash()))
            .unwrap_or_default();
        out.insert(
            (registry.scope(), meta.stream_id, meta.election),
            (meta.event_count, id, hash),
        );
    }
}

/// What a step changed.
#[derive(Debug)]
struct Changes {
    streams: Vec<StreamKey>,
    /// Slots of the sessions that changed.
    sessions: Vec<usize>,
    /// Slots of the sessions that ended.
    ended: Vec<usize>,
    /// Slots of the sessions that survived in another role.
    roles: Vec<usize>,
}

impl Snapshot {
    fn changes(&self, after: &Self) -> Changes {
        let keys: HashSet<&StreamKey> = self.streams.keys().chain(after.streams.keys()).collect();
        let streams = keys
            .into_iter()
            .filter(|key| self.streams.get(key) != after.streams.get(key))
            .copied()
            .collect();
        let sessions = (0..self.sessions.len())
            .filter(|slot| self.sessions[*slot] != after.sessions[*slot])
            .collect();
        let slots = 0..self.roles.len();
        let ended = slots
            .clone()
            .filter(|slot| self.roles[*slot].is_some() && after.roles[*slot].is_none())
            .collect();
        let roles = slots
            .filter(|slot| match (&self.roles[*slot], &after.roles[*slot]) {
                (Some(before), Some(after)) => before != after,
                _ => false,
            })
            .collect();
        Changes {
            streams,
            sessions,
            ended,
            roles,
        }
    }
}

/// Whether a 2xx for this area is fine for this identity.
fn may_serve(area: Area, identity: &Identity) -> bool {
    match (area, identity) {
        (Public, _) => true,
        (_, Identity::Anonymous) => false,
        (Ses, _) | (Pg | PgOnly, Identity::Pg { .. }) | (Csb, Identity::Csb { .. }) => true,
        (Pg, Identity::Csb { paper }) => paper.is_some(),
        (PgOnly | Csb, _) => false,
    }
}

impl World {
    async fn snapshot(&self) -> Snapshot {
        let mut streams = HashMap::new();
        record(&self.state.store_registry, &mut streams).await;
        record(&self.state.csb_store_registry, &mut streams).await;
        record(&self.state.pre_submission_store_registry, &mut streams).await;
        record(&self.state.csb_main_store_registry, &mut streams).await;
        let mut sessions = Vec::new();
        let mut roles = Vec::new();
        for token in &self.tokens {
            let session = self.session(Some(token)).await;
            roles.push(session.as_ref().map(|s| match &s.user {
                SessionUser::PoliticalGroup { stream_id, .. } => stream_id.to_string(),
                SessionUser::CentralElectoralCommittee { .. } => "committee".to_string(),
            }));
            sessions
                .push(session.map(|s| format!("{:?}|{:?}|{}", s.user, s.locale, s.csrf_token().0)));
        }
        Snapshot {
            streams,
            sessions,
            roles,
        }
    }

    /// Group streams the committee imported (for examination or the
    /// pre-submission check).
    async fn imported_sources(&self) -> HashSet<StreamId> {
        let mut sources = HashSet::new();
        for stream in self.csb_streams().await {
            let Some(store) = self.csb_store(stream) else {
                continue;
            };
            for event in &store.snapshot().events {
                if let CsbAction::Import {
                    source_stream_id, ..
                } = &event.payload.action
                {
                    sources.insert(*source_stream_id);
                }
            }
        }
        sources
    }

    /// Markers `identity` must never see.
    async fn forbidden_markers(&self, identity: &Identity) -> Vec<&str> {
        let imported = self.imported_sources().await;
        let mut forbidden = Vec::new();
        for (index, actor) in self.pg.iter().enumerate() {
            let (secret, appellation) = match identity {
                Identity::Pg {
                    actor: Some(own), ..
                } => (*own != index, *own != index),
                // The monitoring page lists every warm group's appellation.
                Identity::Csb { .. } => (!imported.contains(&actor.stream), false),
                _ => (true, true),
            };
            if secret {
                forbidden.push(actor.secret.as_str());
            }
            if appellation {
                forbidden.push(actor.appellation.as_str());
            }
        }
        forbidden
    }

    async fn check_markers(&self, viewer: &Viewer, observed: &Observed) -> Result<(), String> {
        for marker in self.forbidden_markers(&viewer.identity).await {
            if let Some(at) = observed.text.find(&marker.to_lowercase()) {
                return Err(format!(
                    "leaked marker {marker}: ...{}...",
                    snippet(&observed.text, at)
                ));
            }
        }
        Ok(())
    }

    /// Writes only land where the requester may write, and never without a
    /// valid CSRF token on a POST.
    fn check_writes(
        &self,
        step: &Step,
        viewer: &Viewer,
        forged: bool,
        changes: &Changes,
    ) -> Result<(), String> {
        if let Some(slot) = changes.roles.first() {
            return Err(format!("the session in slot {slot} changed role"));
        }
        // Finding, see `finding_tokenless_login_post_ends_the_session`: only
        // ending a session, only the requester's own (checked below).
        let login = step.route().path == "/login"
            && changes.streams.is_empty()
            && changes.sessions == changes.ended;
        if forged && !login && (!changes.streams.is_empty() || !changes.sessions.is_empty()) {
            return Err(format!(
                "POST without a valid CSRF token changed {changes:?}"
            ));
        }
        for (scope, stream, election) in &changes.streams {
            let allowed = match &viewer.identity {
                Identity::Pg { stream: own, .. } => {
                    *scope == Scope::PoliticalGroup && stream == own
                }
                Identity::Csb { .. } => *scope != Scope::PoliticalGroup,
                Identity::Anonymous => false,
            };
            if !allowed {
                return Err(format!("changed {scope:?} stream {stream} ({election:?})"));
            }
        }
        let own_slot = match viewer.identity {
            Identity::Anonymous => None,
            _ => step.who.slot(),
        };
        match changes
            .sessions
            .iter()
            .find(|slot| Some(**slot) != own_slot)
        {
            Some(slot) => Err(format!("changed the session in slot {slot}")),
            None => Ok(()),
        }
    }

    async fn check(
        &self,
        step: &Step,
        viewer: &Viewer,
        observed: &Observed,
        forged: bool,
        changes: &Changes,
    ) -> Result<(), String> {
        let route = step.route();
        // The test auth service holds no keys to sign SP metadata with.
        let unsigned_metadata = route.path == "/saml/sp/metadata"
            && observed.status == StatusCode::INTERNAL_SERVER_ERROR;
        if observed.status.is_server_error() && !unsigned_metadata {
            return Err("server error".to_string());
        }
        if observed.status.is_success() && !may_serve(route.area, &viewer.identity) {
            return Err(format!("{:?} route answered 2xx", route.area));
        }
        self.check_markers(viewer, observed).await?;
        self.check_writes(step, viewer, forged, changes)
    }

    async fn run(&self, step: &Step, coverage: &Mutex<Coverage>) -> Result<(), String> {
        let before = self.snapshot().await;
        let viewer = self.viewer(step.who).await;
        let prepared = self.prepare(step, &viewer).await;
        let uri = prepared.request.uri().to_string();
        let response = self
            .app
            .clone()
            .oneshot(prepared.request)
            .await
            .expect("infallible");
        let observed = observe(response).await;
        self.settle().await;
        let changes = before.changes(&self.snapshot().await);
        let forged =
            prepared.post && (prepared.presented.is_none() || prepared.presented != viewer.csrf);
        coverage
            .lock()
            .expect("coverage")
            .add(step, &viewer.identity, &observed, &changes);
        self.check(step, &viewer, &observed, forged, &changes)
            .await
            .map_err(|err| {
                format!(
                    "{err}\n  step: {step:?}\n  uri: {uri}\n  as: {:?}\n  status: {}",
                    viewer.identity, observed.status
                )
            })
    }
}

async fn run_case(case: &Case, brp: &BrpClient, coverage: &Mutex<Coverage>) -> Result<(), String> {
    let world = World::new(case.seed, brp).await;
    for (index, step) in case.steps.iter().enumerate() {
        world
            .run(step, coverage)
            .await
            .map_err(|err| format!("step {index}: {err}"))?;
    }
    Ok(())
}

/// What a run exercised, printed at its end (see `--nocapture`).
#[derive(Default)]
struct Coverage {
    /// Responses per (area and identity, method, status class).
    responses: BTreeMap<(String, &'static str, u16), usize>,
    /// Steps that changed a stream, per area and identity.
    writes: BTreeMap<String, usize>,
}

impl Coverage {
    fn add(&mut self, step: &Step, identity: &Identity, observed: &Observed, changes: &Changes) {
        let kind = match identity {
            Identity::Anonymous => "anon",
            Identity::Pg { .. } => "group",
            Identity::Csb { paper: None } => "committee",
            Identity::Csb { paper: Some(_) } => "paper",
        };
        let area = format!("{:?} as {kind}", step.route().area);
        let method = if step.is_post() { "POST" } else { "GET" };
        *self
            .responses
            .entry((area.clone(), method, observed.status.as_u16() / 100))
            .or_default() += 1;
        if !changes.streams.is_empty() {
            *self.writes.entry(area).or_default() += 1;
        }
    }
}

impl fmt::Display for Coverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total: usize = self.responses.values().sum();
        writeln!(f, "access rule fuzzer: {total} requests")?;
        for ((area, method, class), count) in &self.responses {
            writeln!(f, "  {area:<20} {method:<4} {class}xx: {count}")?;
        }
        for (area, count) in &self.writes {
            writeln!(f, "  {area:<20} steps that wrote: {count}")?;
        }
        Ok(())
    }
}

/// Run `steps` against a fresh world, as the fuzzer does.
fn replay(steps: Vec<Step>) -> Result<(), String> {
    runtime().block_on(async {
        let brp = BrpStub::serving(Vec::new()).await;
        run_case(&Case { seed: 0, steps }, &brp.client, &Mutex::default()).await
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn access_rules_hold_for_random_request_sequences() {
    let runtime = runtime();
    let brp = runtime.block_on(BrpStub::serving(Vec::new()));
    let mut config = proptest_config(DEFAULT_CASES);
    config.failure_persistence = None;
    config.max_shrink_iters = 4096;

    let coverage = Mutex::new(Coverage::default());

    let result = TestRunner::new(config).run(&case(), |case| {
        runtime
            .block_on(run_case(&case, &brp.client, &coverage))
            .map_err(TestCaseError::fail)
    });

    eprintln!("{}", coverage.lock().expect("coverage"));
    match result {
        Ok(()) => {}
        Err(TestError::Fail(reason, case)) => {
            panic!("{reason}\nminimal failing case:\n{case:#?}")
        }
        Err(err) => panic!("{err}"),
    }
}

/// The oracle sees what it looks for: the owner's marker in a page and in a
/// ZIP download, the imported group's marker on the committee side, and a
/// leak when the same response is judged for another group.
#[test]
fn access_rule_oracle_sees_markers() {
    let runtime = runtime();
    runtime.block_on(async {
        let brp = BrpStub::serving(Vec::new()).await;
        let world = World::new(7, &brp.client).await;
        let a = world.viewer(Who::A).await;
        let b = world.viewer(Who::B).await;
        let csb = world.viewer(Who::Csb).await;
        let hash = world
            .state
            .store_registry
            .get_cached(world.pg[0].stream, world.pg[0].election)
            .map(|store| EventHashPrefix::of(&store.current_event_hash()))
            .expect("store");
        let imported = world.csb_streams().await[0];
        let list = world.stream_ids(imported).lists[0].clone();
        let secret = world.pg[0].secret.to_lowercase();

        for (viewer, uri) in [
            (&a, "/persons".to_string()),
            (&a, format!("/generate/{hash}/nl/documents.zip")),
            (&csb, format!("/csb/examination/{imported}/list/{list}")),
        ] {
            let request = Request::builder()
                .uri(&uri)
                .header(
                    header::COOKIE,
                    format!(
                        "{SESSION_COOKIE_NAME}={}",
                        viewer.cookie.clone().expect("cookie")
                    ),
                )
                .body(Body::empty())
                .expect("request");
            let observed =
                observe(world.app.clone().oneshot(request).await.expect("response")).await;
            assert_eq!(observed.status, StatusCode::OK, "{uri}");
            assert!(observed.text.contains(&secret), "{uri} lacks the marker");
            assert!(
                world.check_markers(viewer, &observed).await.is_ok(),
                "{uri}"
            );
            assert!(world.check_markers(&b, &observed).await.is_err(), "{uri}");
        }
    });
}

/// The write and area checks flag what they look for.
#[test]
fn access_rule_oracle_flags_violations() {
    runtime().block_on(async {
        let brp = BrpStub::serving(Vec::new()).await;
        let world = World::new(3, &brp.client).await;
        let b = world.viewer(Who::B).await;
        let csb = world.viewer(Who::Csb).await;
        let stream = |actor: &PgActor| (Scope::PoliticalGroup, actor.stream, actor.election);
        let (a_stream, b_stream) = (stream(&world.pg[0]), stream(&world.pg[1]));
        let step = |who| Step {
            who,
            narrow: true,
            aligned: true,
            route: 0,
            post: true,
            params: [Pick::Fresh; 4],
            query: 0,
            csrf: Csrf::Own,
            transport: Transport::Form,
            payload: Payload::Empty,
            fetch: Fetch::Absent,
        };
        let changes = |streams: Vec<StreamKey>, sessions: Vec<usize>| Changes {
            streams,
            sessions,
            ended: Vec::new(),
            roles: Vec::new(),
        };
        let writes = |who, viewer, forged, streams, sessions| {
            world.check_writes(&step(who), viewer, forged, &changes(streams, sessions))
        };

        assert!(writes(Who::B, &b, false, vec![b_stream], vec![1]).is_ok());
        assert!(writes(Who::B, &b, false, vec![a_stream], vec![]).is_err());
        assert!(writes(Who::Csb, &csb, false, vec![a_stream], vec![]).is_err());
        assert!(writes(Who::B, &b, false, vec![], vec![0]).is_err());
        assert!(writes(Who::B, &b, true, vec![b_stream], vec![]).is_err());
        assert!(!may_serve(Csb, &b.identity));
        assert!(!may_serve(Pg, &csb.identity));
        assert!(!may_serve(Ses, &Identity::Anonymous));
    });
}

/// Shrunk from the fuzzer: `A POST /login csrf=Missing via=Form`. `/login`
/// takes no CSRF token (there is no session yet), and while the IdP metadata
/// is unavailable `handle_login` fails with `end_session: true`, ending the
/// session the cookie names. Without fetch metadata (a legacy browser) a
/// cross-site form can so log a user out.
#[test]
#[ignore = "finding: a POST /login without CSRF token ends the caller's session while the IdP metadata is unavailable"]
fn finding_tokenless_login_post_ends_the_session() {
    runtime().block_on(async {
        let brp = BrpStub::serving(Vec::new()).await;
        let world = World::new(0, &brp.client).await;
        let a = world.viewer(Who::A).await;
        let request = Request::builder()
            .method("POST")
            .uri("/login")
            .header(
                header::COOKIE,
                format!(
                    "{SESSION_COOKIE_NAME}={}",
                    a.cookie.clone().expect("cookie")
                ),
            )
            .header(header::CONTENT_TYPE, URLENCODED)
            .body(Body::empty())
            .expect("request");

        world.app.clone().oneshot(request).await.expect("response");

        assert!(
            world.session(a.cookie.as_deref()).await.is_some(),
            "a POST without a CSRF token ended the session"
        );
    });
}

/// Shrunk from the fuzzer:
/// `Paper POST /political-group/information csrf=Own via=Form body=Junk(0)`,
/// then `Csb GET /csb/examination/overzicht-kandidatenlijsten.pdf`. The form's
/// fields are optional, so `a=b` clears the imported group's appellation in
/// paper corrections, after which the committee's overview answers 500.
#[test]
#[ignore = "finding: clearing the appellation in paper corrections makes the candidate list overview answer 500"]
fn finding_cleared_appellation_breaks_the_candidate_list_overview() {
    runtime().block_on(async {
        let brp = BrpStub::serving(Vec::new()).await;
        let world = World::new(0, &brp.client).await;
        let paper = world.viewer(Who::Paper).await;
        let csb = world.viewer(Who::Csb).await;
        let cookie = |viewer: &Viewer| {
            format!(
                "{SESSION_COOKIE_NAME}={}",
                viewer.cookie.clone().expect("cookie")
            )
        };

        let clear = Request::builder()
            .method("POST")
            .uri("/political-group/information")
            .header(header::COOKIE, cookie(&paper))
            .header(header::CONTENT_TYPE, URLENCODED)
            .body(Body::from(format!(
                "a=b&csrf_token={}",
                paper.csrf.clone().expect("csrf")
            )))
            .expect("request");
        let response = world.app.clone().oneshot(clear).await.expect("response");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let imported = world.csb_streams().await[0];
        let mut failed = Vec::new();
        for uri in [
            "/csb/examination/overzicht-kandidatenlijsten.pdf".to_string(),
            format!("/csb/examination/{imported}/verzuimbrief.pdf"),
        ] {
            let request = Request::builder()
                .uri(&uri)
                .header(header::COOKIE, cookie(&csb))
                .body(Body::empty())
                .expect("request");
            let response = world.app.clone().oneshot(request).await.expect("response");
            if response.status().is_server_error() {
                failed.push(format!("{uri}: {}", response.status()));
            }
        }
        assert!(failed.is_empty(), "{failed:?}");
    });
}

/// Shrunk from the fuzzer. The add-omission handler stores the `{reference}`
/// path segment unchecked, so a candidate omission can name no candidate
/// (here the group's stream); the group's omissions page then answers 500.
#[test]
#[ignore = "finding: a candidate omission naming no candidate makes the omissions page answer 500"]
fn finding_omission_for_an_unknown_candidate_breaks_the_omissions_page() {
    let result = replay(vec![
        Step::replay(
            Who::Csb,
            true,
            "/csb/examination/{stream_id}/omission/{omission_type}/{reference}",
            &[Pick::Own(0), Pick::Fresh, Pick::Own(0)],
            Payload::Plausible(0),
        ),
        Step::replay(
            Who::Csb,
            false,
            "/csb/examination/{stream_id}/omissions",
            &[Pick::Own(0)],
            Payload::Empty,
        ),
    ]);
    if let Err(err) = result {
        panic!("{err}");
    }
}
