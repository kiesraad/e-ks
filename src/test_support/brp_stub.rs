//! A stub BRP, so `cargo test` stays hermetic.

use std::{net::SocketAddr, sync::Arc};

use axum::{Json, Router, extract::State, routing::post};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};

use crate::{constants, structs::brp::BrpClient};

#[derive(Clone)]
struct StubState {
    persons: Arc<Vec<Value>>,
    queries: Arc<Mutex<Vec<Value>>>,
}

/// A BRP holding a canned list of `personen`, answering both the lookup by
/// burgerservicenummer and the search on personal details the way the real one
/// does, and recording the queries it was sent.
pub struct BrpStub {
    pub client: BrpClient,
    queries: Arc<Mutex<Vec<Value>>>,
    server: JoinHandle<()>,
}

impl BrpStub {
    pub async fn serving(persons: Vec<Value>) -> Self {
        let queries = Arc::new(Mutex::new(Vec::new()));
        let state = StubState {
            persons: Arc::new(persons),
            queries: Arc::clone(&queries),
        };

        let router = Router::new()
            .route(
                &format!("/{}", constants::BRP_PERSONS_ENDPOINT),
                post(
                    |State(state): State<StubState>, Json(query): Json<Value>| async move {
                        let persons = matching(&state.persons, &query);
                        state.queries.lock().push(query.clone());
                        Json(json!({
                            "type": query["type"].clone(),
                            "personen": persons,
                        }))
                    },
                ),
            )
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        Self {
            client: BrpClient::new_for_test(&format!("http://{addr}")),
            queries,
            server,
        }
    }

    /// How many requests the stub was sent.
    pub fn query_count(&self) -> usize {
        self.queries.lock().len()
    }

    /// The queries of one `type`, in the order they were sent.
    pub fn queries_of_type(&self, query_type: &str) -> Vec<Value> {
        self.queries
            .lock()
            .iter()
            .filter(|query| query["type"] == query_type)
            .cloned()
            .collect()
    }
}

/// The records this query selects, matching the real BRP: the lookup returns
/// the requested burgerservicenummers, the search matches `geslachtsnaam` and
/// `geboortedatum` exactly and narrows on the optional parameters, leaving
/// deceased people out unless they were asked for.
fn matching(persons: &[Value], query: &Value) -> Vec<Value> {
    match query["type"].as_str() {
        Some("RaadpleegMetBurgerservicenummer") => {
            let wanted = query["burgerservicenummer"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            persons
                .iter()
                .filter(|person| wanted.contains(&person["burgerservicenummer"]))
                .cloned()
                .collect()
        }
        Some("ZoekMetGeslachtsnaamEnGeboortedatum") => persons
            .iter()
            .filter(|person| {
                let matches_optional = |parameter: &str, value: &Value| {
                    query[parameter].is_null() || query[parameter] == *value
                };

                person["naam"]["geslachtsnaam"] == query["geslachtsnaam"]
                    && person["geboorte"]["datum"]["datum"] == query["geboortedatum"]
                    && matches_optional("voorvoegsel", &person["naam"]["voorvoegsel"])
                    && matches_optional("geslacht", &person["geslacht"]["code"])
                    && (query["inclusiefOverledenPersonen"] == json!(true)
                        || person["overlijden"].is_null())
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

/// A record matching [`crate::test_utils::sample_person_from_brp`] on every
/// checked field, so a test only changes the one thing it is about.
pub fn matching_record(bsn: &str) -> Value {
    json!({
        "burgerservicenummer": bsn,
        "naam": {
            "geslachtsnaam": "Bruin",
            "voorvoegsel": "de",
            "voorletters": "T.",
        },
        "geslacht": { "code": "V" },
        "geboorte": { "datum": { "datum": "1990-12-11" } },
        "nationaliteiten": [ { "nationaliteit": { "code": "0001" } } ],
        "uitsluitingKiesrecht": { "uitgeslotenVanKiesrecht": false },
        "verblijfplaats": {
            "type": "Adres",
            "verblijfadres": { "woonplaats": "Utrecht" },
        },
    })
}

impl Drop for BrpStub {
    fn drop(&mut self) {
        self.server.abort();
    }
}
