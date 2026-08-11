-- Phase 4 — 알림, 운영, 통계, 개인정보 파기 (§15, §11.5, §12.5, §17.3, §18, §19).
--
-- The Phase 1 migration already created every table this phase writes to:
-- `notifications`, `notification_templates`, `notification_deliveries`,
-- `push_subscriptions`, `admin_cases`, `audit_logs`, `analytics_daily_store` and
-- `deletion_ledger`. What is missing there are the columns, keys and seed rows that only
-- matter once a message is actually sent, a case is actually worked, or a subject actually
-- asks to be erased — so this migration adds those rather than reshaping anything.
--
-- sqlx wraps each migration in its own transaction, so this file must not open one.

-- ---------------------------------------------------------------------------
-- 앱 내 알림 (§15.1-1) — 모든 거래·운영 사건의 기준 기록
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.notifications
    -- §18.3: API → outbox → job → provider delivery 에 같은 correlation_id.
    ADD COLUMN IF NOT EXISTS correlation_id uuid,
    ADD COLUMN IF NOT EXISTS priority varchar(16) NOT NULL DEFAULT 'NORMAL',
    -- The consumer may clear their inbox. §15.1 is explicit that this is a *view* over
    -- what happened, so dismissing one changes nothing about the coupon or the ledger —
    -- which is exactly why the row is flagged rather than deleted.
    ADD COLUMN IF NOT EXISTS dismissed_at timestamptz,
    ADD COLUMN IF NOT EXISTS source_event_type varchar(120);

ALTER TABLE coupon.notifications
    DROP CONSTRAINT IF EXISTS ck_notifications_priority;
ALTER TABLE coupon.notifications
    ADD CONSTRAINT ck_notifications_priority CHECK (
        priority IN ('URGENT', 'HIGH', 'NORMAL', 'LOW')
    );

-- The inbox is cursor-paginated on `(occurred_at, id)` (§11.1), which the Phase 1 index
-- (keyed on `read_at`) cannot serve.
CREATE INDEX IF NOT EXISTS ix_notifications_user_feed
    ON coupon.notifications (user_id, occurred_at DESC, id DESC);

-- ---------------------------------------------------------------------------
-- 전달 결과 (§15.4, NOTIFY-003, NOTIFY-004)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.notification_deliveries
    ADD COLUMN IF NOT EXISTS correlation_id uuid,
    -- Which §15.3 purpose this send was judged under. Stored rather than re-derived so a
    -- later template reclassification cannot rewrite the basis a past send was made on.
    ADD COLUMN IF NOT EXISTS purpose varchar(64) NOT NULL DEFAULT 'TRANSACTIONAL',
    -- §15.2: 템플릿 변경은 과거 발송 재현을 위해 기존 버전을 보존한다. `template_id`
    -- already points at an immutable version row; these two make the send self-describing
    -- without a join, which is what an incident review actually reads.
    ADD COLUMN IF NOT EXISTS template_code varchar(100),
    ADD COLUMN IF NOT EXISTS template_version_no integer,
    ADD COLUMN IF NOT EXISTS provider varchar(64),
    -- The variables that were escaped and handed to the provider. §15.2 forbids user HTML
    -- in the payload, so what is recorded here is the allow-listed set, not the message.
    ADD COLUMN IF NOT EXISTS rendered_variables jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS suppression_reason varchar(64),
    -- NOTIFY-004 야간 정책: a marketing send inside the quiet hours waits for the next
    -- permitted moment rather than being dropped.
    ADD COLUMN IF NOT EXISTS scheduled_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    -- The notification's own expiry, copied so the worker can decide "this can no longer
    -- arrive in time" without a join (NOTIFY-004 마지막 줄).
    ADD COLUMN IF NOT EXISTS deliver_before timestamptz,
    ADD COLUMN IF NOT EXISTS max_attempts integer NOT NULL DEFAULT 5,
    -- NOTIFY-004: `event_id + channel + template_version + recipient` 를 발송 고유키로
    -- 쓴다. `uq_notification_delivery_channel` is one row per notification per channel,
    -- which is a *weaker* statement: two notifications could exist for one event if a
    -- caller ever wrote one directly. This key says the thing the scenario says.
    ADD COLUMN IF NOT EXISTS dedupe_key text;

ALTER TABLE coupon.notification_deliveries
    DROP CONSTRAINT IF EXISTS ck_notification_delivery_max_attempts;
ALTER TABLE coupon.notification_deliveries
    ADD CONSTRAINT ck_notification_delivery_max_attempts CHECK (max_attempts >= 1);

-- A suppressed delivery must say why. "We did not send" without a reason is unauditable,
-- and §15.3 makes consent the thing being audited.
ALTER TABLE coupon.notification_deliveries
    DROP CONSTRAINT IF EXISTS ck_notification_delivery_suppression;
ALTER TABLE coupon.notification_deliveries
    ADD CONSTRAINT ck_notification_delivery_suppression CHECK (
        status <> 'SUPPRESSED' OR suppression_reason IS NOT NULL
    );

CREATE UNIQUE INDEX IF NOT EXISTS uq_notification_deliveries_dedupe
    ON coupon.notification_deliveries (dedupe_key)
    WHERE dedupe_key IS NOT NULL;

DROP INDEX IF EXISTS coupon.ix_notification_deliveries_work;
CREATE INDEX IF NOT EXISTS ix_notification_deliveries_work
    ON coupon.notification_deliveries (status, COALESCE(next_attempt_at, scheduled_at))
    WHERE status IN ('PENDING', 'FAILED_RETRYABLE');

-- §18.4 provider 실패율 is read per channel over a recent window.
CREATE INDEX IF NOT EXISTS ix_notification_deliveries_channel_status
    ON coupon.notification_deliveries (channel, status, updated_at DESC);

-- §15.4: provider callback 은 서명과 provider reference 를 검증하고 멱등 처리한다.
--
-- Kept as its own table rather than a counter on the delivery: the same callback arriving
-- twice must be *recognised*, and recognising it means having stored the first one. The
-- unique key is the provider's own event id, which is what a duplicate re-uses.
CREATE TABLE IF NOT EXISTS coupon.notification_delivery_callbacks (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    delivery_id uuid REFERENCES coupon.notification_deliveries(id) ON DELETE RESTRICT,
    channel coupon.notification_channel NOT NULL,
    provider varchar(64) NOT NULL,
    provider_event_id varchar(255) NOT NULL,
    provider_reference varchar(255) NOT NULL,
    reported_status varchar(64) NOT NULL,
    -- Whether the signature verified. A rejected callback is still recorded: SEC-002's
    -- reasoning applies here too — the attempts are evidence.
    signature_valid boolean NOT NULL,
    -- The provider's own timestamp, used for the replay window.
    signed_at timestamptz NOT NULL,
    applied boolean NOT NULL DEFAULT false,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_notification_callback_event UNIQUE (channel, provider, provider_event_id)
);

CREATE INDEX IF NOT EXISTS ix_notification_callbacks_delivery
    ON coupon.notification_delivery_callbacks (delivery_id, received_at DESC);

-- ---------------------------------------------------------------------------
-- FCM Web Push subscriptions (§15.1-2, NOTIFY-003)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.push_subscriptions
    ADD COLUMN IF NOT EXISTS disabled_at timestamptz,
    ADD COLUMN IF NOT EXISTS disabled_reason varchar(64),
    ADD COLUMN IF NOT EXISTS last_success_at timestamptz;

-- ---------------------------------------------------------------------------
-- 알림 템플릿 (§15.2)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.notification_templates
    -- §15.2 approval status is already here; this records *when* the provider decided, so
    -- a rejection can be told apart from one that was never submitted.
    ADD COLUMN IF NOT EXISTS provider_approved_at timestamptz,
    ADD COLUMN IF NOT EXISTS priority varchar(16) NOT NULL DEFAULT 'NORMAL',
    ADD COLUMN IF NOT EXISTS retired_at timestamptz;

ALTER TABLE coupon.notification_templates
    DROP CONSTRAINT IF EXISTS ck_notification_template_approval;
ALTER TABLE coupon.notification_templates
    ADD CONSTRAINT ck_notification_template_approval CHECK (
        provider_approval_status IS NULL
        OR provider_approval_status IN ('NOT_REQUIRED', 'PENDING', 'APPROVED', 'REJECTED')
    );

-- 알림톡 must name an approved provider template; §15.1 forbids using the ordinary friend
-- message API as a stand-in for one.
ALTER TABLE coupon.notification_templates
    DROP CONSTRAINT IF EXISTS ck_notification_template_alimtalk;
ALTER TABLE coupon.notification_templates
    ADD CONSTRAINT ck_notification_template_alimtalk CHECK (
        channel <> 'KAKAO_ALIMTALK'
        OR NOT active
        OR (provider_template_id IS NOT NULL AND provider_approval_status = 'APPROVED')
    );

-- Version 1 of every §15.2 event, for every channel it may travel on.
--
-- Bodies are placeholder-only: `{{name}}` slots are filled from an allow-list and escaped
-- at render time, so no user-supplied text ever reaches a provider as markup (§15.2).
-- 알림톡 rows land as PENDING approval and are therefore inactive: until a real provider
-- template id exists, the constraint above refuses to let them go live.
INSERT INTO coupon.notification_templates
    (code, version_no, locale, channel, purpose, provider_template_id,
     provider_approval_status, subject_template, body_template, variable_schema, active,
     priority)
VALUES
    ('STAMP_EARNED', 1, 'ko-KR', 'IN_APP', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 도장 {{quantity}}개 적립',
     '{{store_name}}에서 도장 {{quantity}}개가 적립되었습니다. 목표까지 {{remaining}}개 남았습니다.',
     '["store_name","quantity","remaining","expires_at"]', true, 'NORMAL'),
    ('STAMP_EARNED', 1, 'ko-KR', 'FCM_WEB_PUSH', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 도장 {{quantity}}개 적립',
     '목표까지 {{remaining}}개 남았습니다.',
     '["store_name","quantity","remaining"]', true, 'NORMAL'),
    ('REWARD_ISSUED', 1, 'ko-KR', 'IN_APP', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 리워드가 발급되었습니다',
     '{{benefit}} 리워드를 받았습니다. {{expires_at}}까지 사용할 수 있습니다.',
     '["store_name","benefit","expires_at"]', true, 'HIGH'),
    ('REWARD_ISSUED', 1, 'ko-KR', 'FCM_WEB_PUSH', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 리워드가 발급되었습니다',
     '{{benefit}} · {{expires_at}}까지',
     '["store_name","benefit","expires_at"]', true, 'HIGH'),
    ('COUPON_ISSUED', 1, 'ko-KR', 'IN_APP', 'MARKETING', NULL, 'NOT_REQUIRED',
     '{{store_name}} 쿠폰이 도착했습니다',
     '{{campaign_name}} · {{benefit}}. {{expires_at}}까지 사용할 수 있습니다.',
     '["store_name","campaign_name","benefit","expires_at"]', true, 'NORMAL'),
    ('COUPON_ISSUED', 1, 'ko-KR', 'FCM_WEB_PUSH', 'MARKETING', NULL, 'NOT_REQUIRED',
     '{{store_name}} 쿠폰이 도착했습니다',
     '{{campaign_name}} · {{expires_at}}까지',
     '["store_name","campaign_name","expires_at"]', true, 'NORMAL'),
    ('COUPON_EXPIRING', 1, 'ko-KR', 'IN_APP', 'INFORMATIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 쿠폰이 곧 만료됩니다',
     '{{benefit}} 쿠폰의 사용 기간이 {{days_left}}일 남았습니다.',
     '["store_name","benefit","days_left","expires_at"]', true, 'NORMAL'),
    ('COUPON_EXPIRING', 1, 'ko-KR', 'FCM_WEB_PUSH', 'INFORMATIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 쿠폰이 곧 만료됩니다',
     '{{days_left}}일 남았습니다.',
     '["store_name","days_left"]', true, 'NORMAL'),
    ('COUPON_USED', 1, 'ko-KR', 'IN_APP', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 쿠폰을 사용했습니다',
     '{{used_at}} · {{discount_amount}}원 할인 (거래 {{transaction_id}})',
     '["store_name","used_at","discount_amount","transaction_id"]', true, 'HIGH'),
    ('COUPON_USED', 1, 'ko-KR', 'FCM_WEB_PUSH', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 쿠폰을 사용했습니다',
     '{{discount_amount}}원 할인',
     '["store_name","discount_amount"]', true, 'HIGH'),
    ('TRANSACTION_VOIDED', 1, 'ko-KR', 'IN_APP', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 거래가 취소되었습니다',
     '{{detail}}',
     '["store_name","detail","restored"]', true, 'HIGH'),
    ('TRANSACTION_VOIDED', 1, 'ko-KR', 'FCM_WEB_PUSH', 'TRANSACTIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 거래가 취소되었습니다',
     '{{detail}}',
     '["store_name","detail"]', true, 'HIGH'),
    ('STORE_SUSPENDED', 1, 'ko-KR', 'IN_APP', 'INFORMATIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}} 이용이 일시 중단되었습니다',
     '{{detail}} 문의는 고객센터로 부탁드립니다.',
     '["store_name","detail"]', true, 'HIGH'),
    ('STORE_CLOSED', 1, 'ko-KR', 'IN_APP', 'INFORMATIONAL', NULL, 'NOT_REQUIRED',
     '{{store_name}}이 영업을 종료했습니다',
     '{{detail}} 보유하신 혜택 처리 방법은 고객센터로 문의해 주세요.',
     '["store_name","detail"]', true, 'HIGH'),
    ('SECURITY_ALERT', 1, 'ko-KR', 'IN_APP', 'SECURITY', NULL, 'NOT_REQUIRED',
     '계정 보안 알림',
     '{{occurred_at}}에 {{detail}} 본인이 아니라면 즉시 모든 기기에서 로그아웃해 주세요.',
     '["occurred_at","detail"]', true, 'URGENT'),
    ('SECURITY_ALERT', 1, 'ko-KR', 'FCM_WEB_PUSH', 'SECURITY', NULL, 'NOT_REQUIRED',
     '계정 보안 알림',
     '{{detail}}',
     '["detail"]', true, 'URGENT')
ON CONFLICT (code, version_no, locale, channel) DO NOTHING;

-- ---------------------------------------------------------------------------
-- 운영: 제재와 세션 폐기 (§11.5, §3.3, ADMIN-002)
-- ---------------------------------------------------------------------------

DO $$ BEGIN
    CREATE TYPE coupon.sanction_type AS ENUM ('TEMPORARY', 'PERMANENT');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.sanction_status AS ENUM (
        'PENDING_APPROVAL', 'ACTIVE', 'EXPIRED', 'LIFTED', 'REJECTED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

CREATE TABLE IF NOT EXISTS coupon.user_sanctions (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    subject_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    case_id uuid NOT NULL REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT,
    sanction_type coupon.sanction_type NOT NULL,
    status coupon.sanction_status NOT NULL DEFAULT 'PENDING_APPROVAL',
    -- ADMIN-002: 제재 대상에게 공개 가능한 사유와 내부 사유를 분리한다.
    public_reason text NOT NULL,
    internal_reason text NOT NULL,
    requested_by_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    requested_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    approved_by_user_id uuid REFERENCES coupon.users(id) ON DELETE RESTRICT,
    approved_at timestamptz,
    effective_from timestamptz NOT NULL DEFAULT clock_timestamp(),
    -- 임시 정지는 종료 시각을 둘 수 있고 만료 시 자동 복구 후보가 된다 (ADMIN-002).
    expires_at timestamptz,
    lifted_at timestamptz,
    lifted_by_user_id uuid REFERENCES coupon.users(id) ON DELETE RESTRICT,
    lift_reason text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_user_sanction_reasons CHECK (
        btrim(public_reason) <> '' AND btrim(internal_reason) <> ''
    ),
    -- A temporary suspension without an end is a permanent one wearing the wrong label.
    CONSTRAINT ck_user_sanction_period CHECK (
        (sanction_type = 'TEMPORARY' AND expires_at IS NOT NULL AND expires_at > effective_from)
        OR (sanction_type = 'PERMANENT' AND expires_at IS NULL)
    ),
    -- §3.3 / ADMIN-002: 영구 정지와 폐쇄는 이중 확인한다. The approver may not be the
    -- requester, and the database is what makes that true rather than a code path.
    CONSTRAINT ck_user_sanction_separation CHECK (
        approved_by_user_id IS NULL OR approved_by_user_id <> requested_by_user_id
    ),
    CONSTRAINT ck_user_sanction_permanent_approval CHECK (
        sanction_type <> 'PERMANENT'
        OR status <> 'ACTIVE'
        OR (approved_by_user_id IS NOT NULL AND approved_at IS NOT NULL)
    ),
    CONSTRAINT ck_user_sanction_lift CHECK (
        status <> 'LIFTED' OR (lifted_at IS NOT NULL AND lifted_by_user_id IS NOT NULL)
    )
);

-- One live sanction per subject: two overlapping suspensions have no coherent meaning,
-- and "which one lifts it" would have no answer.
CREATE UNIQUE INDEX IF NOT EXISTS uq_user_sanctions_active
    ON coupon.user_sanctions (subject_user_id)
    WHERE status IN ('PENDING_APPROVAL', 'ACTIVE');
CREATE INDEX IF NOT EXISTS ix_user_sanctions_expiry
    ON coupon.user_sanctions (expires_at)
    WHERE status = 'ACTIVE' AND expires_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_user_sanctions_case
    ON coupon.user_sanctions (case_id, created_at DESC);

-- §11.5 `POST /admin/users/:id/revoke-sessions`.
--
-- Firebase holds the sessions, but the decision and its reason are ours to keep: the
-- provider call can fail and be retried, and an investigation needs the request either
-- way. `valid_after` is also what the token check compares against, so a revocation is
-- enforced here even while Firebase propagates.
ALTER TABLE coupon.users
    ADD COLUMN IF NOT EXISTS sessions_valid_after timestamptz,
    -- §17.3: 탈퇴자는 거래 원장의 user FK 를 가명 tombstone 으로 치환할 수 있게 한다.
    -- Set when erasure has replaced this row's personal data with a pseudonym; the row
    -- itself stays so the ledger's foreign keys still resolve.
    ADD COLUMN IF NOT EXISTS tombstoned_at timestamptz,
    ADD COLUMN IF NOT EXISTS pseudonym_label varchar(64);

CREATE TABLE IF NOT EXISTS coupon.user_session_revocations (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    subject_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    case_id uuid REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT,
    requested_by_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    reason text NOT NULL,
    valid_after timestamptz NOT NULL,
    provider_result varchar(32) NOT NULL DEFAULT 'PENDING',
    provider_error text,
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_session_revocation_reason CHECK (btrim(reason) <> ''),
    CONSTRAINT ck_session_revocation_result CHECK (
        provider_result IN ('PENDING', 'SUCCEEDED', 'FAILED', 'SKIPPED')
    )
);

CREATE INDEX IF NOT EXISTS ix_session_revocations_subject
    ON coupon.user_session_revocations (subject_user_id, occurred_at DESC);

-- ---------------------------------------------------------------------------
-- 운영: 사건 (§11.5 `/admin/cases`, ADMIN-004)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.admin_cases
    -- ADMIN-004: 해결 방식은 설명, 쿠폰 재발급, 도장 보정, 거래 취소, 제재로 구분한다.
    ADD COLUMN IF NOT EXISTS resolution_type varchar(32),
    ADD COLUMN IF NOT EXISTS resolved_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    -- ADMIN-006: 처리 기한.
    ADD COLUMN IF NOT EXISTS due_at timestamptz,
    ADD COLUMN IF NOT EXISTS correlation_id uuid;

ALTER TABLE coupon.admin_cases
    DROP CONSTRAINT IF EXISTS ck_admin_case_resolution_type;
ALTER TABLE coupon.admin_cases
    ADD CONSTRAINT ck_admin_case_resolution_type CHECK (
        resolution_type IS NULL
        OR resolution_type IN (
            'EXPLANATION', 'COUPON_REISSUE', 'STAMP_ADJUSTMENT', 'TRANSACTION_VOID',
            'SANCTION', 'PRIVACY_ERASURE', 'NO_ACTION'
        )
    );

CREATE INDEX IF NOT EXISTS ix_admin_cases_created
    ON coupon.admin_cases (created_at DESC, id DESC);

-- ---------------------------------------------------------------------------
-- 감사 검색 (§11.5 `/admin/audit-logs`, §12.5 변조 탐지)
-- ---------------------------------------------------------------------------

CREATE INDEX IF NOT EXISTS ix_audit_logs_time
    ON coupon.audit_logs (occurred_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS ix_audit_logs_action
    ON coupon.audit_logs (action, occurred_at DESC);
CREATE INDEX IF NOT EXISTS ix_audit_logs_case
    ON coupon.audit_logs (case_id, occurred_at DESC)
    WHERE case_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- 통계 (§6.3, §19 분석)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.analytics_daily_store
    -- §19: 취소·보정은 순수치와 총 발생치를 모두 제공한다. `stamp_earned_count` is the
    -- gross figure and `stamp_voided_count` the reversal, so both readings are available
    -- without either being a subtraction the caller has to guess at.
    ADD COLUMN IF NOT EXISTS stamp_transaction_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS campaign_coupon_revoked_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS redemption_voided_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS discount_amount_total bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS aggregated_job_id uuid REFERENCES coupon.job_registry(id) ON DELETE SET NULL;

ALTER TABLE coupon.analytics_daily_store
    DROP CONSTRAINT IF EXISTS ck_analytics_extra_counts;
ALTER TABLE coupon.analytics_daily_store
    ADD CONSTRAINT ck_analytics_extra_counts CHECK (
        stamp_transaction_count >= 0 AND campaign_coupon_revoked_count >= 0
        AND redemption_voided_count >= 0 AND discount_amount_total >= 0
    );

CREATE INDEX IF NOT EXISTS ix_analytics_daily_store_day
    ON coupon.analytics_daily_store (store_id, business_day DESC);

-- ---------------------------------------------------------------------------
-- 개인정보 보존·파기 (§17.3, ADMIN-006)
-- ---------------------------------------------------------------------------

-- §17.3: 프로필, 동의, 거래, 감사, 민원, 보안 로그별 보존기간을 *설정 테이블*로 관리한다.
-- §23.2 leaves the actual durations to the pre-launch legal review, so the seeds below are
-- starting values an operator changes without a deploy — not constants in code.
CREATE TABLE IF NOT EXISTS coupon.retention_policies (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    data_category varchar(64) NOT NULL UNIQUE,
    retention_days integer NOT NULL,
    -- Why this number: the statute or contract that fixes it, so a change is reviewable.
    legal_basis text NOT NULL,
    -- Whether records in this category may be erased on request at all, or whether a
    -- statutory retention keeps them until the period runs out (§17.3).
    erasable_on_request boolean NOT NULL DEFAULT true,
    active boolean NOT NULL DEFAULT true,
    updated_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_retention_days CHECK (retention_days BETWEEN 1 AND 36500),
    CONSTRAINT ck_retention_basis CHECK (btrim(legal_basis) <> '')
);

INSERT INTO coupon.retention_policies (data_category, retention_days, legal_basis, erasable_on_request)
VALUES
    ('PROFILE', 30, '탈퇴 후 재가입 남용 방지를 위한 최소 보관. §23.2 법률 검토 전 잠정값.', true),
    ('CONSENT', 1825, '동의·철회 증빙 보관. §23.2 법률 검토 전 잠정값.', false),
    ('TRANSACTION', 1825, '전자상거래 등에서의 소비자보호에 관한 법률상 거래기록 보존. §23.2 확인 필요.', false),
    ('AUDIT', 1095, '관리자 행위 감사 추적 (§12.5, SEC-005). §23.2 법률 검토 전 잠정값.', false),
    ('CASE', 1095, '민원 처리 이력 (ADMIN-004). §23.2 법률 검토 전 잠정값.', false),
    ('SECURITY_LOG', 365, '통신비밀보호법상 접속기록 보존. §23.2 확인 필요.', false),
    ('NOTIFICATION', 180, '발송 이력·수신 거부 증빙. §23.2 법률 검토 전 잠정값.', true)
ON CONFLICT (data_category) DO NOTHING;

ALTER TABLE coupon.deletion_ledger
    -- The pseudonym the subject's rows now carry. Recording it here rather than only on
    -- `users` is what lets the ledger be replayed against a restored backup (§17.3,
    -- §18.5): the replay must reproduce the *same* tombstone, not invent a new one.
    ADD COLUMN IF NOT EXISTS pseudonym_label varchar(64),
    ADD COLUMN IF NOT EXISTS applied_scopes jsonb NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS reapplied_at timestamptz,
    ADD COLUMN IF NOT EXISTS reapply_count integer NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS blocked_reason text,
    ADD COLUMN IF NOT EXISTS job_id uuid REFERENCES coupon.job_registry(id) ON DELETE SET NULL;

ALTER TABLE coupon.deletion_ledger
    DROP CONSTRAINT IF EXISTS ck_deletion_ledger_reapply;
ALTER TABLE coupon.deletion_ledger
    ADD CONSTRAINT ck_deletion_ledger_reapply CHECK (reapply_count >= 0);

-- The subject id is not a foreign key on purpose: after a restore the ledger has to be
-- replayable even for a row the restore brought back, and the point of the replay is that
-- the ledger outlives what it erases.
CREATE INDEX IF NOT EXISTS ix_deletion_ledger_subject
    ON coupon.deletion_ledger (subject_user_id, requested_at DESC);

-- ---------------------------------------------------------------------------
-- campaign_counters.reserved_count 제거
-- ---------------------------------------------------------------------------
--
-- Phase 3 left `reserved_count` and `global_reserved_count` permanently zero, and that is
-- the correct behaviour rather than a missing feature: §13.2's claim issues the coupon and
-- bumps the counter in *one* transaction, so there is no window in which a slot is spoken
-- for but not yet issued. `issued_count` already counts PENDING instances, which covers
-- the direct-issue worker's in-flight rows too (§8.4).
--
-- A column that is structurally always zero is worse than no column: every quantity check
-- reads it, every reviewer has to work out that it cannot matter, and the day someone
-- writes to it the invariant it appears in silently changes meaning. So it goes.
ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_global_counts;
ALTER TABLE coupon.campaigns
    DROP COLUMN IF EXISTS global_reserved_count;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_global_counts CHECK (
        global_issued_count >= 0 AND global_revoked_count >= 0
        AND global_issued_count <= COALESCE(total_quantity, unlimited_total_cap, 1000000)
    );

ALTER TABLE coupon.campaign_counters
    DROP CONSTRAINT IF EXISTS ck_campaign_counter_nonnegative;
ALTER TABLE coupon.campaign_counters
    DROP COLUMN IF EXISTS reserved_count;
ALTER TABLE coupon.campaign_counters
    ADD CONSTRAINT ck_campaign_counter_nonnegative CHECK (
        issued_count >= 0 AND revoked_count >= 0
    );

-- ---------------------------------------------------------------------------
-- Timestamps for the new tables
-- ---------------------------------------------------------------------------

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['user_sanctions', 'retention_policies']
    LOOP
        EXECUTE format('DROP TRIGGER IF EXISTS trg_%I_updated_at ON coupon.%I', table_name, table_name);
        EXECUTE format(
            'CREATE TRIGGER trg_%I_updated_at BEFORE UPDATE ON coupon.%I '
            'FOR EACH ROW EXECUTE FUNCTION coupon.set_updated_at()',
            table_name, table_name
        );
    END LOOP;
END;
$$;

-- Consent evidence and session revocations are records of decisions, not mutable state.
DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['user_session_revocations']
    LOOP
        EXECUTE format('DROP TRIGGER IF EXISTS trg_%I_append_only ON coupon.%I', table_name, table_name);
        EXECUTE format(
            'CREATE TRIGGER trg_%I_append_only BEFORE DELETE ON coupon.%I '
            'FOR EACH ROW EXECUTE FUNCTION coupon.reject_append_only_mutation()',
            table_name, table_name
        );
    END LOOP;
END;
$$;

COMMENT ON TABLE coupon.notification_delivery_callbacks IS
    'Provider delivery callbacks; unique per provider event id so a replay is recognised (§15.4)';
COMMENT ON TABLE coupon.retention_policies IS
    'Per-category retention periods (§17.3); operational configuration, not code constants';
COMMENT ON TABLE coupon.deletion_ledger IS
    'Erasure record replayed after a backup restore so a purged subject cannot return (§17.3, §18.5)';
COMMENT ON COLUMN coupon.notification_deliveries.dedupe_key IS
    'event_id + channel + template_version + recipient — the NOTIFY-004 send key';

INSERT INTO coupon.schema_metadata (key, value)
VALUES ('schema_version', '20260813000400')
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    updated_at = clock_timestamp();
