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
run, 3 reorders, EK27, base URL `http://localhost:3000`.

## What each session does

The flow lives in [src/scenario.rs](src/scenario.rs) as a flat top-to-bottom
script. **That's the only file you need to touch when the actions a user does
change.** Per session:

1. `GET /login` (no TVS on `main` — login creates a session immediately)
2. `GET /select-election`, `POST /select-election` with `election=EK27`
3. Browse `/persons`, `/political-group`, `/candidate-lists`
4. For each fixture row in `persons.csv`: `GET /persons/create`,
   `POST /persons/create`, `GET /persons/{id}/address`,
   `POST /persons/{id}/address`
5. `POST /political-group` (display + legal name)
6. `POST /political-group/authorised-agents/create`
7. `POST /political-group/list-submitter/update`
8. Two `POST /political-group/substitute-submitters/create`
9. `POST /candidate-lists/create` with all 16 EK27 districts
10. `POST /candidate-lists/{id}/add` with `action=add-all`
11. `POST /candidate-lists/{id}/reorder` (JSON, `--reorders` times with
    a fresh shuffle each)
12. `GET /submit`, then one `GET` per download endpoint:
    `eml210.eml.xml`, `h1.pdf`, `h3_1.pdf`, `h4.pdf`, `h9.zip`
13. Final survey of `/persons` and `/candidate-lists`

The CSRF token is sniffed once from the first rendered page and reused for the
rest of the session — it doesn't rotate.

## Output

```
method   label                               count        p50        p90        p99        max   errors
--------------------------------------------------------------------------------------------------
POST     person-create:post                    600      9.3ms     11.0ms     18.4ms     26.9ms        0
POST     person-address:post                   580      9.0ms     10.7ms     16.8ms     18.9ms        0
POST     candidate-list:reorder                 60     10.4ms     11.9ms     12.5ms     12.6ms        0
GET      download:h1                            20      5.7ms      6.6ms      7.0ms      7.0ms       20
...
total requests: 4380, errors: 100
wall clock: 1.59s
```

`errors` counts HTTP ≥400. PDF/zip downloads will return 500 unless the
`typst-webservice` is reachable; eml210 doesn't need it. Form re-renders
(validation errors that come back as 200) are logged to stderr as
`skip <name>: …` and the session continues with the next candidate —
submitting forms with errors is realistic user behaviour.

## Layout

- `src/main.rs` — CLI, spawns N user tasks
- `src/client.rs` — `Client` per user: cookie store, GET/POST/JSON-POST/download
- `src/scenario.rs` — the per-session flow
- `src/data.rs` — loads `../src/fixtures/persons.csv` (the same CSV the server
  uses to seed fixtures)
- `src/metrics.rs` — async metric channel + summary
