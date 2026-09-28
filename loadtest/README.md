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
run, 10 edits, 3 reorders, EK27, `nl` documents, no think time, dev login, base
URL `http://localhost:3000`.

By default each session logs in through `/dev/login`, so the server has to be
built with the `dev-features` feature (it is on by default). A server without
it, such as the preview environment, takes `--login tvs-mock` instead: each
session then runs the real SAML flow against the TVS mock the server is
configured with (https://tvs-mock.eks-test.nl), logging in with a random BSN:

```bash
cargo run --release --manifest-path loadtest/Cargo.toml -- \
    --base-url https://preview.kandidaatstellen.nl \
    --login tvs-mock \
    --users 20 \
    --think-time-ms 1000 \
    --think-time-jitter-ms 2000
```

The BSN is drawn from the whole nine-digit range, not just the `999…` numbers
the mock's own "Nieuwe test-BSN" button picks: a BSN that an earlier run used
logs into that run's stream, where every person create fails the uniqueness
check.

If the server runs with `EKS_KEY` set, pass `--eks-key` (or export `EKS_KEY`),
otherwise every request answers 401. It is only sent to `--base-url`'s origin,
never to the mock. Preview needs none: requests through its CDN already pass.

`--think-time-ms` makes every user wait that long before each request they
make themselves, plus a uniformly random extra of up to
`--think-time-jitter-ms`. Redirect hops and the SAML auto-submit POST go out
immediately, as a browser sends them. Both default to 0, i.e. every user
hammers the server back to back.

`--continuous` keeps `--users` sessions running at all times: as soon as a
user's session finishes, that user starts a new one with a fresh login (and so
a fresh stream, and with `--login tvs-mock` a fresh BSN). It runs until
`--duration-secs` has passed or you press Ctrl-C. Every request is logged to
stderr as it completes (time, user and run, method, status, duration, label,
path), so the summary on stdout stays clean. `--duration-secs` and Ctrl-C also
work without `--continuous`: the sessions still running are aborted and the
summary covers everything recorded up to that point.

`--persons-per-user` is capped at 80, the app's `MAX_CANDIDATES`: every person
a session creates also goes onto its candidate list, and
`CandidateList::update_order` rejects a longer list outright.

## What each session does

The flow lives in [src/scenario.rs](src/scenario.rs) as a flat top-to-bottom
script, with the login in [src/login.rs](src/login.rs). **Those are the only
files you need to touch when the actions a user does change.** Per session:

1. Log in, minting a fresh stream per session so concurrent users never share a
   store. `--login dev`: `GET /dev/login?select_election=true`. `--login
   tvs-mock`: `GET /login`, `POST /login` (renders the SAML auto-submit form),
   `POST` the `SAMLRequest` to the mock (renders its BSN form), `POST` a random
   BSN to the mock (302 to the ACS), `GET /saml/sp/acs?SAMLart=…`
2. `GET /select-election`, `POST /select-election` with `election=EK27`
3. Browse `/persons`, `/political-group`, `/political-group/information`,
   `/audit-log`, `/candidate-lists`
4. For each of `--persons-per-user` fixture rows in `persons.csv`:
   `GET /persons/create`, `POST /persons/create`, `GET /persons/{id}/address`,
   `POST /persons/{id}/address`
5. `POST /persons/{id}/update` for the first `--edits` of them, resending the
   full personal-data form with an amended last name
6. `POST /political-group` (list designation), then
   `POST /political-group/information` (the group's `appellation`)
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

Steps 6 to 9 mirror what `src/fixtures/political_groups.rs` seeds, down to the
same names and addresses.

Notes on things the client has to get right for the app to accept it:

- The session's CSRF token is **not** fixed for the session's lifetime:
  `/select-election` rotates it, so a token sniffed once and reused 400s every
  later POST. Each step therefore reads the token off the page it just
  rendered, exactly like a browser submitting that page's form. Form POSTs
  carry it in the body; the JSON reorder POST carries it in the
  `x-csrf-token` header, because `auth::csrf_guard` never reads a token out of
  a JSON payload.
- Sessions are pinned to the `User-Agent` that created them, so every request
  sends the same one.
- `POST /persons/{id}/update` submits the *whole* personal-data form: it is
  `#[serde(default)]` server-side, so a partial POST silently clears date of
  birth, BSN and place of residence, and the candidate then drops out of the
  models with "Missing birth date for candidate".
- The CSV writes a last name the way it appears on a candidate list, prefix
  included ("de Goede"), but `LastName` rejects a value whose first word is a
  known prefix. `PersonRow::last_name_parts` splits the two into `last_name`
  and `last_name_prefix`, the same way the server's fixture loader does. The
  prefix table is read straight out of
  `src/structs/common/last_name_prefix.rs`, so it cannot drift.
- The electoral-district checkbox values are `ElectoralDistrict::serde_name()`,
  i.e. the variant name (`NoordHolland`), *not* the district code (`prov8`).
  The enum is generated by the app's build script.
- The group's `appellation` has to be set before step 13: `pg_appellation()`
  errors for anything but a blank list when it is unset, which fails the whole
  download. (The field used to be called `display_name`.)
- Rate limits are per stream and each session gets its own, so the defaults
  (3000 events and 60 downloads per hour) are far out of reach for one
  session. Raise `RATE_LIMIT_EVENTS` / `RATE_LIMIT_DOWNLOADS` if you point
  many runs at a single stream.

`--load-fixtures` is of limited use: the fixtures are seeded from the same
`persons.csv` this test reads, and `uniqueness_errors` rejects a duplicate BSN
outright, so every person the session tries to create is refused and steps 10
to 13 have nothing to work with.

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
sessions: 20 completed, 0 failed, 0 aborted
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
submitting forms with errors is realistic user behaviour. A clean run against
the current fixtures reports no skips at all.

## Layout

- `src/main.rs` — CLI, spawns N user tasks
- `src/client.rs` — `Client` per user: cookie store, GET/POST/JSON-POST/download,
  think time, and the CSRF token of the last page rendered
- `src/login.rs`: dev login and the SAML login through the TVS mock
- `src/scenario.rs` — the per-session flow
- `src/data.rs` — loads `../src/fixtures/persons.csv` (the same CSV the server
  uses to seed fixtures) and splits last-name prefixes off its rows
- `src/metrics.rs` — async metric channel + summary
