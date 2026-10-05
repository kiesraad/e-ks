-- Full application database schema. Mirrors the runtime migration in
-- src/store/database.rs (streams, events, sessions, pending requests, passkey
-- accounts and passkeys) plus the ACME challenge storage.

-- Event stream bookkeeping: one row per (stream, election) pair.
CREATE TABLE IF NOT EXISTS streams (
  stream_id UUID NOT NULL,
  election TEXT NOT NULL,
  last_event_id BIGINT NOT NULL,
  scope TEXT NOT NULL DEFAULT 'political_group',
  encrypted_key BYTEA,
  PRIMARY KEY (stream_id, election)
);

-- Encrypted, hash-chained event log.
CREATE TABLE IF NOT EXISTS events (
  stream_id UUID NOT NULL,
  election TEXT NOT NULL,
  event_id BIGINT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  hash BYTEA NOT NULL,
  payload BYTEA NOT NULL,
  PRIMARY KEY (stream_id, election, event_id)
);
-- Supports looking up an event (and its stream) by its chain hash.
CREATE INDEX IF NOT EXISTS events_hash_idx ON events(hash);

-- User sessions. `token` holds the token's SHA-256 hash, not the token itself;
-- `identity` holds the serialized `SessionUser` (who the session belongs to).
CREATE TABLE IF NOT EXISTS sessions (
  token TEXT PRIMARY KEY,
  identity JSONB NOT NULL,
  locale TEXT NOT NULL,
  last_activity TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  user_agent_hash TEXT,
  csrf_token TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_last_activity_idx
  ON sessions(last_activity);

-- In-flight request deduplication.
CREATE TABLE IF NOT EXISTS pending_requests (
  id TEXT PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS pending_requests_created_at_idx
  ON pending_requests(created_at);

-- Passkey accounts for the CSB login: the WebAuthn user handle, the name a
-- committee member types at login (unique ignoring case) and who created it
-- (a serialized `CsbUser`).
CREATE TABLE IF NOT EXISTS csb_passkey_accounts (
  id UUID PRIMARY KEY,
  name TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_by JSONB NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS csb_passkey_accounts_name_idx
  ON csb_passkey_accounts (lower(name));

-- Registered passkeys: the webauthn-rs credential (public key, counter and
-- backup flags) as JSON, under a member-chosen label. Go with their account.
CREATE TABLE IF NOT EXISTS csb_passkeys (
  id UUID PRIMARY KEY,
  account_id UUID NOT NULL REFERENCES csb_passkey_accounts(id) ON DELETE CASCADE,
  credential_id BYTEA NOT NULL UNIQUE,
  label TEXT NOT NULL,
  passkey JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS csb_passkeys_account_id_idx
  ON csb_passkeys (account_id);

-- http-01 challenge tokens, shared so any instance can answer a validation
-- request. The key authorization is public by protocol.
CREATE TABLE IF NOT EXISTS acme_challenges (
  token TEXT PRIMARY KEY,
  key_authorization TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS acme_challenges_created_at_idx
  ON acme_challenges(created_at);
