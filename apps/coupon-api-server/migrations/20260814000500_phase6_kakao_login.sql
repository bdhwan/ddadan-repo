-- 카카오 OIDC 로그인 (§9.2, §11.2, AUTH-002, AUTH-003).
--
-- sqlx wraps each migration in its own transaction, so this file must not open or
-- close one itself.
--
-- Three short-lived tables, all of them server-side state that must not live in the
-- browser:
--
--   * `oauth_login_sessions`  — state / nonce / PKCE verifier between authorize and
--     callback. §9.2-2 allows an encrypted cookie or short-lived server storage; server
--     storage is chosen because it also makes `state` single-use, which a cookie cannot
--     enforce on its own.
--   * `oauth_exchange_codes`  — the one-time code the callback hands back (§9.2-3). It
--     carries the *verified provider identity*, never an access or refresh token: §9.2
--     requires the Kakao tokens to be discarded once login completes.
--   * `provider_webhook_events` — the dedupe key that makes the unlink webhook
--     idempotent and replay-proof (§9.2 마지막).
--
-- Nothing here is a record of what happened; `audit_logs` keeps that. These rows are
-- work-in-progress and are swept once they expire.

CREATE TABLE IF NOT EXISTS coupon.oauth_login_sessions (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    provider coupon.auth_provider NOT NULL,
    -- The `state` itself is a bearer value: whoever holds it can complete the login. It
    -- is stored hashed for the same reason a password is.
    state_hash bytea NOT NULL,
    nonce text NOT NULL,
    -- AES-256-GCM sealed (§16.5). A verifier readable from a database dump would defeat
    -- the point of PKCE.
    code_verifier_ciphertext bytea NOT NULL,
    redirect_uri text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CONSTRAINT uq_oauth_login_session_state UNIQUE (provider, state_hash),
    CONSTRAINT ck_oauth_login_session_period CHECK (expires_at > created_at)
);

CREATE INDEX IF NOT EXISTS ix_oauth_login_sessions_expiry
    ON coupon.oauth_login_sessions (expires_at);

CREATE TABLE IF NOT EXISTS coupon.oauth_exchange_codes (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    provider coupon.auth_provider NOT NULL,
    code_hash bytea NOT NULL,
    -- Kakao's `sub`. The provider identity is settled by the time this row exists; the
    -- internal user is not, because §9.2-6 may still have to create one and AUTH-003 may
    -- route the same code into an explicit link instead.
    provider_subject varchar(255) NOT NULL,
    email_ciphertext bytea,
    email_verified boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CONSTRAINT uq_oauth_exchange_code UNIQUE (provider, code_hash),
    CONSTRAINT ck_oauth_exchange_code_period CHECK (expires_at > created_at)
);

CREATE INDEX IF NOT EXISTS ix_oauth_exchange_codes_expiry
    ON coupon.oauth_exchange_codes (expires_at);

CREATE TABLE IF NOT EXISTS coupon.provider_webhook_events (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    provider coupon.auth_provider NOT NULL,
    event_type text NOT NULL,
    -- The provider's own event identifier when it sends one, otherwise a digest of the
    -- signed body. Either way, receiving the same event twice must change nothing.
    event_key text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_provider_webhook_event UNIQUE (provider, event_type, event_key)
);

CREATE INDEX IF NOT EXISTS ix_provider_webhook_events_received
    ON coupon.provider_webhook_events (received_at);
