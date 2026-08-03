# loadtest

Concurrent session load test for the e-KS app. Spawns N simulated users, each
of which logs in and walks the full happy-path flow with realistic GETs in
between every form submission.

## Run

Start the server (Postgres + the eks binary), then:

```bash
cargo run --release --manifest-path loadtest/Cargo.toml -- \
    --base-url http://127.0.0.1:3000 \
    --users 100 \
    --persons-per-user 50 \
    --reorders 10
```

`--help` lists every option. Defaults: 10 users, 1 run each, 50 persons per
run, 10 edits, 3 reorders, EK27, `nl` documents, base URL
`http://localhost:3000`.

The server has to be built with the `dev-features` feature (it is on by
default): the real login is DigiD/TVS SAML, which a load test can't drive, so
each session logs in through `/dev/login` instead. Pass `--eks-key` if the
server runs with `EKS_KEY` set, otherwise every request answers 401.

## What each session does

The flow lives in [src/scenario.rs](src/scenario.rs) as a flat top-to-bottom
script. **That's the only file you need to touch when the actions a user does
change.** Per session:

1. `GET /dev/login?select_election=true` — creates a session whose stream is
   derived from a random BSN, so concurrent users never share a store
2. `GET /select-election`, `POST /select-election` with `election=EK27`
3. Browse `/persons`, `/political-group`, `/political-group/information`,
   `/audit-log`, `/candidate-lists`
4. For each of `--persons-per-user` fixture rows in `persons.csv`:
   `GET /persons/create`, `POST /persons/create`, `GET /persons/{id}/address`,
   `POST /persons/{id}/address`
5. `POST /persons/{id}/update` for the first `--edits` of them, resending the
   full personal-data form with an amended last name
6. `POST /political-group` (list designation), then
   `POST /political-group/information` (display name)
7. `POST /political-group/name-authorisation/create` (holds the legal name)
8. `POST /political-group/list-submitter/update`
9. Two `POST /political-group/substitute-submitters/create`
10. `POST /candidate-lists/create` with all 16 EK27 districts
11. `POST /candidate-lists/{id}/add` with `action=add-all`
12. `POST /candidate-lists/{id}/reorder` (JSON, `--reorders` times with
    a fresh shuffle each)
13. `GET /finalise`, then the single all-in-one download
    `GET /generate/nl/documents.zip`, then `POST /hide-download-warning`
14. Final survey of `/persons` and `/candidate-lists`

Notes on things the client has to get right for the app to accept it:

- The CSRF token is sniffed once from the first rendered page and reused for the
  rest of the session — it doesn't rotate. Form POSTs carry it in the body; the
  JSON reorder POST carries it in the `x-csrf-token` header, because
  `auth::csrf_guard` never reads a token out of a JSON payload.
- Sessions are pinned to the `User-Agent` that created them, so every request
  sends the same one.
- `POST /persons/{id}/update` submits the *whole* personal-data form: it is
  `#[serde(default)]` server-side, so a partial POST silently clears date of
  birth, BSN and place of residence, and the candidate then drops out of the
  models with "Missing birth date for candidate".

## Output

```
method   label                               count        p50        p90        p99        max   errors
--------------------------------------------------------------------------------------------------
POST     person-create:post                    600      9.3ms     11.0ms     18.4ms     26.9ms        0
POST     person-address:post                   580      9.0ms     10.7ms     16.8ms     18.9ms        0
POST     candidate-list:reorder                 60     10.4ms     11.9ms     12.5ms     12.6ms        0
GET      download:documents                     20    298.6ms      1.2s       2.1s       2.1s        0
...
total requests: 4380, errors: 100
wall clock: 1.59s
```

`errors` counts HTTP ≥400. `download:documents` is timed as a full transfer and
is normally the slowest leg by a wide margin: PDF rendering happens in-process
(`textris-pdf`, on a blocking thread) and produces one H9 per candidate, so it
competes with request handling for the app's own CPU. That is the main thing
this test is here to measure — bump `--timeout-secs` if you see
"send failed after X.Xs" on it.

Form re-renders (validation errors that come back as 200) are logged to stderr
as `skip <name>: …` and the session continues with the next candidate —
submitting forms with errors is realistic user behaviour.

## Layout

- `src/main.rs` — CLI, spawns N user tasks
- `src/client.rs` — `Client` per user: cookie store, GET/POST/JSON-POST/download
- `src/scenario.rs` — the per-session flow
- `src/data.rs` — loads `../src/fixtures/persons.csv` (the same CSV the server
  uses to seed fixtures)
- `src/metrics.rs` — async metric channel + summary
