-- sqlx wraps each migration in its own transaction, so this file must not open or
-- close one itself.

CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;
CREATE SCHEMA IF NOT EXISTS coupon;

COMMENT ON SCHEMA coupon IS 'DDADAN coupon, loyalty, wallet, notification, and operations domain';

DO $$ BEGIN
    CREATE TYPE coupon.user_status AS ENUM (
        'PENDING_VERIFICATION', 'ACTIVE', 'SUSPENDED', 'WITHDRAWAL_PENDING', 'WITHDRAWN'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.auth_provider AS ENUM ('FIREBASE_PASSWORD', 'KAKAO');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.auth_identity_status AS ENUM ('ACTIVE', 'UNLINKED', 'REVOKED');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.account_role AS ENUM (
        'CONSUMER', 'STORE_OWNER', 'SUPPORT', 'OPERATIONS', 'SECURITY', 'SUPER_ADMIN'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.consent_action AS ENUM ('GRANTED', 'REVOKED');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.store_status AS ENUM (
        'DRAFT', 'PENDING_REVIEW', 'ACTIVE', 'SUSPENDED', 'CLOSED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.review_status AS ENUM (
        'PENDING', 'APPROVED', 'CHANGES_REQUESTED', 'REJECTED', 'CANCELLED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.resource_status AS ENUM ('ACTIVE', 'INACTIVE', 'ARCHIVED');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.policy_status AS ENUM ('DRAFT', 'SCHEDULED', 'ACTIVE', 'PAUSED', 'ENDED');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.benefit_type AS ENUM ('FIXED_AMOUNT', 'PERCENTAGE', 'FREE_ITEM');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.campaign_status AS ENUM (
        'DRAFT', 'SCHEDULED', 'ISSUING', 'PAUSED', 'ENDED', 'CANCELLED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.issue_mode AS ENUM ('DIRECT', 'FIRST_COME');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.coupon_source_type AS ENUM ('CAMPAIGN', 'LOYALTY_REWARD', 'ADMIN_GRANT');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.coupon_status AS ENUM (
        'PENDING', 'AVAILABLE', 'RESERVED', 'USED', 'EXPIRED', 'REVOKED', 'VOIDED', 'ISSUE_FAILED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.stamp_transaction_status AS ENUM (
        'CONFIRMED', 'VOIDED', 'REQUIRES_ADMIN_REVIEW'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.stamp_ledger_event_type AS ENUM (
        'EARN', 'CONSUME_FOR_REWARD', 'EXPIRE', 'VOID', 'ADMIN_ADJUSTMENT'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.reservation_status AS ENUM ('ACTIVE', 'CONFIRMED', 'CANCELLED', 'EXPIRED', 'REVOKED');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.redemption_status AS ENUM ('CONFIRMED', 'VOIDED', 'REQUIRES_ADMIN_REVIEW');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.notification_channel AS ENUM ('IN_APP', 'FCM_WEB_PUSH', 'KAKAO_ALIMTALK');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.notification_delivery_status AS ENUM (
        'PENDING', 'SENDING', 'DELIVERED', 'FAILED_RETRYABLE', 'FAILED_PERMANENT', 'SUPPRESSED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.outbox_status AS ENUM ('PENDING', 'PUBLISHED', 'FAILED');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.job_status AS ENUM (
        'PENDING_OUTBOX', 'QUEUED', 'RUNNING', 'RETRY_WAIT', 'PAUSE_REQUESTED',
        'PAUSED', 'SUCCEEDED', 'DEAD_LETTER', 'CANCELLED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.admin_case_type AS ENUM (
        'STORE_REVIEW', 'COUPON_MISSING', 'WRONG_REDEMPTION', 'QR_ABUSE',
        'WRONG_STAMP', 'STORE_CLOSURE', 'SECURITY_INCIDENT', 'PRIVACY_REQUEST', 'OTHER'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.admin_case_status AS ENUM (
        'OPEN', 'INVESTIGATING', 'WAITING_CUSTOMER', 'WAITING_STORE', 'RESOLVED', 'CLOSED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.admin_adjustment_status AS ENUM (
        'DRAFT', 'PENDING_APPROVAL', 'APPROVED', 'EXECUTING', 'SUCCEEDED', 'REJECTED', 'FAILED'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE coupon.actor_type AS ENUM ('USER', 'STORE_OWNER', 'SYSTEM_ADMIN', 'SYSTEM', 'PROVIDER');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

CREATE OR REPLACE FUNCTION coupon.set_updated_at()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.updated_at := clock_timestamp();
    NEW.version := OLD.version + 1;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION coupon.reject_append_only_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'table %.% is append-only', TG_TABLE_SCHEMA, TG_TABLE_NAME
        USING ERRCODE = '55000';
END;
$$;

CREATE TABLE IF NOT EXISTS coupon.schema_metadata (
    key text PRIMARY KEY,
    value text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE IF NOT EXISTS coupon.users (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    firebase_uid varchar(128) NOT NULL UNIQUE,
    consumer_key uuid NOT NULL DEFAULT public.gen_random_uuid() UNIQUE,
    status coupon.user_status NOT NULL DEFAULT 'PENDING_VERIFICATION',
    display_name varchar(100) NOT NULL,
    primary_email_ciphertext bytea,
    primary_email_lookup_hash bytea,
    email_verified_at timestamptz,
    suspended_at timestamptz,
    suspension_reason text,
    withdrawal_requested_at timestamptz,
    withdrawn_at timestamptz,
    last_login_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_users_display_name_not_blank CHECK (btrim(display_name) <> ''),
    CONSTRAINT ck_users_status_timestamps CHECK (
        (status <> 'WITHDRAWN' OR withdrawn_at IS NOT NULL)
        AND (status <> 'SUSPENDED' OR suspended_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS ix_users_status ON coupon.users (status);
CREATE INDEX IF NOT EXISTS ix_users_email_lookup_hash
    ON coupon.users (primary_email_lookup_hash)
    WHERE primary_email_lookup_hash IS NOT NULL;

CREATE TABLE IF NOT EXISTS coupon.auth_identities (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    provider coupon.auth_provider NOT NULL,
    provider_subject varchar(255) NOT NULL,
    status coupon.auth_identity_status NOT NULL DEFAULT 'ACTIVE',
    linked_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    unlinked_at timestamptz,
    provider_profile_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_auth_identity_provider_subject UNIQUE (provider, provider_subject),
    CONSTRAINT ck_auth_identity_unlinked_at CHECK (
        (status = 'ACTIVE' AND unlinked_at IS NULL)
        OR (status <> 'ACTIVE' AND unlinked_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS ix_auth_identities_user ON coupon.auth_identities (user_id, status);

CREATE TABLE IF NOT EXISTS coupon.user_roles (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    role coupon.account_role NOT NULL,
    granted_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    granted_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    revoked_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    revoked_at timestamptz,
    revoke_reason text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_user_roles_revoke_fields CHECK (
        (revoked_at IS NULL AND revoked_by_user_id IS NULL AND revoke_reason IS NULL)
        OR revoked_at IS NOT NULL
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_user_roles_active
    ON coupon.user_roles (user_id, role)
    WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS coupon.terms_documents (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    document_type varchar(64) NOT NULL,
    version_label varchar(32) NOT NULL,
    locale varchar(16) NOT NULL DEFAULT 'ko-KR',
    title varchar(200) NOT NULL,
    content_uri text NOT NULL,
    content_sha256 char(64) NOT NULL,
    required boolean NOT NULL DEFAULT true,
    effective_at timestamptz NOT NULL,
    retired_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_terms_document UNIQUE (document_type, version_label, locale),
    CONSTRAINT ck_terms_document_period CHECK (retired_at IS NULL OR retired_at > effective_at)
);

CREATE TABLE IF NOT EXISTS coupon.consent_events (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    document_id uuid REFERENCES coupon.terms_documents(id) ON DELETE RESTRICT,
    scope varchar(100) NOT NULL,
    store_id uuid,
    channel coupon.notification_channel,
    action coupon.consent_action NOT NULL,
    source varchar(64) NOT NULL,
    ip_hash bytea,
    user_agent_class varchar(64),
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT ck_consent_scope_not_blank CHECK (btrim(scope) <> ''),
    CONSTRAINT ck_consent_source_not_blank CHECK (btrim(source) <> '')
);

CREATE INDEX IF NOT EXISTS ix_consent_events_user_scope_time
    ON coupon.consent_events (user_id, scope, occurred_at DESC);

CREATE TABLE IF NOT EXISTS coupon.stores (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    owner_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    status coupon.store_status NOT NULL DEFAULT 'DRAFT',
    name varchar(200) NOT NULL,
    slug varchar(100) NOT NULL,
    business_type varchar(100),
    description text,
    contact_phone_ciphertext bytea,
    contact_phone_lookup_hash bytea,
    address_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    timezone varchar(64) NOT NULL DEFAULT 'Asia/Seoul',
    business_day_cutoff time NOT NULL DEFAULT '00:00:00',
    business_hours jsonb NOT NULL DEFAULT '{}'::jsonb,
    temporary_suspension_policy jsonb NOT NULL DEFAULT '{}'::jsonb,
    activated_at timestamptz,
    suspended_at timestamptz,
    closed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_stores_slug UNIQUE (slug),
    CONSTRAINT ck_stores_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT ck_stores_slug_format CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,98}[a-z0-9]$'),
    CONSTRAINT ck_stores_status_timestamps CHECK (
        (status <> 'ACTIVE' OR activated_at IS NOT NULL)
        AND (status <> 'SUSPENDED' OR suspended_at IS NOT NULL)
        AND (status <> 'CLOSED' OR closed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_stores_active_owner
    ON coupon.stores (owner_user_id)
    WHERE status <> 'CLOSED';
CREATE INDEX IF NOT EXISTS ix_stores_status ON coupon.stores (status);

ALTER TABLE coupon.consent_events
    DROP CONSTRAINT IF EXISTS fk_consent_events_store;
ALTER TABLE coupon.consent_events
    ADD CONSTRAINT fk_consent_events_store
    FOREIGN KEY (store_id) REFERENCES coupon.stores(id) ON DELETE RESTRICT;

CREATE TABLE IF NOT EXISTS coupon.store_business_profiles (
    store_id uuid PRIMARY KEY REFERENCES coupon.stores(id) ON DELETE CASCADE,
    registration_no_ciphertext bytea NOT NULL,
    registration_no_lookup_hash bytea NOT NULL,
    representative_name_ciphertext bytea NOT NULL,
    business_address_ciphertext bytea,
    evidence_object_key text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS ix_store_business_registration_hash
    ON coupon.store_business_profiles (registration_no_lookup_hash);

CREATE TABLE IF NOT EXISTS coupon.store_reviews (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    status coupon.review_status NOT NULL DEFAULT 'PENDING',
    submission_snapshot jsonb NOT NULL,
    submitted_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    reviewer_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    decided_at timestamptz,
    public_reason text,
    internal_reason text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_store_review_decision CHECK (
        (status = 'PENDING' AND decided_at IS NULL)
        OR (status <> 'PENDING' AND decided_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_store_reviews_pending_store
    ON coupon.store_reviews (store_id)
    WHERE status = 'PENDING';
CREATE INDEX IF NOT EXISTS ix_store_reviews_queue
    ON coupon.store_reviews (status, submitted_at);

CREATE TABLE IF NOT EXISTS coupon.store_customers (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    alias varchar(80) NOT NULL,
    first_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    interaction_count bigint NOT NULL DEFAULT 0,
    risk_flags jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_store_customers_store_user UNIQUE (store_id, user_id),
    CONSTRAINT ck_store_customers_interaction_count CHECK (interaction_count >= 0)
);

CREATE INDEX IF NOT EXISTS ix_store_customers_store_last_seen
    ON coupon.store_customers (store_id, last_seen_at DESC);

CREATE TABLE IF NOT EXISTS coupon.favorite_stores (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    favorited_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    removed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_favorite_stores_active
    ON coupon.favorite_stores (user_id, store_id)
    WHERE removed_at IS NULL;
CREATE INDEX IF NOT EXISTS ix_favorite_stores_store
    ON coupon.favorite_stores (store_id, favorited_at DESC)
    WHERE removed_at IS NULL;

CREATE TABLE IF NOT EXISTS coupon.notification_preferences (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    store_id uuid REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    purpose varchar(64) NOT NULL,
    channel coupon.notification_channel NOT NULL,
    enabled boolean NOT NULL DEFAULT false,
    source_consent_event_id uuid REFERENCES coupon.consent_events(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_notification_preference_purpose CHECK (btrim(purpose) <> '')
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_notification_preferences_scope
    ON coupon.notification_preferences (user_id, COALESCE(store_id, '00000000-0000-0000-0000-000000000000'::uuid), purpose, channel);

CREATE TABLE IF NOT EXISTS coupon.catalog_categories (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    name varchar(120) NOT NULL,
    status coupon.resource_status NOT NULL DEFAULT 'ACTIVE',
    sort_order integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_catalog_categories_store_name UNIQUE (store_id, name),
    CONSTRAINT ck_catalog_categories_name CHECK (btrim(name) <> '')
);

CREATE TABLE IF NOT EXISTS coupon.catalog_items (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    category_id uuid REFERENCES coupon.catalog_categories(id) ON DELETE SET NULL,
    sku varchar(100),
    name varchar(160) NOT NULL,
    reference_price bigint,
    status coupon.resource_status NOT NULL DEFAULT 'ACTIVE',
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_catalog_items_name CHECK (btrim(name) <> ''),
    CONSTRAINT ck_catalog_items_reference_price CHECK (reference_price IS NULL OR reference_price >= 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_catalog_items_store_sku
    ON coupon.catalog_items (store_id, sku)
    WHERE sku IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_catalog_items_store_status
    ON coupon.catalog_items (store_id, status, name);

CREATE TABLE IF NOT EXISTS coupon.loyalty_policies (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    version_no integer NOT NULL,
    status coupon.policy_status NOT NULL DEFAULT 'DRAFT',
    name varchar(160) NOT NULL,
    target_stamp_count smallint NOT NULL DEFAULT 10,
    stamps_per_order smallint NOT NULL DEFAULT 1,
    minimum_order_amount bigint NOT NULL DEFAULT 0,
    daily_earning_limit smallint,
    duplicate_warning_seconds integer NOT NULL DEFAULT 300,
    stamp_validity_days smallint NOT NULL DEFAULT 180,
    starts_at timestamptz,
    ends_at timestamptz,
    eligible_item_ids uuid[] NOT NULL DEFAULT '{}',
    eligible_category_ids uuid[] NOT NULL DEFAULT '{}',
    excluded_item_ids uuid[] NOT NULL DEFAULT '{}',
    rule_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    schema_version integer NOT NULL DEFAULT 1,
    published_at timestamptz,
    created_by_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_loyalty_policies_store_version UNIQUE (store_id, version_no),
    CONSTRAINT ck_loyalty_policy_name CHECK (btrim(name) <> ''),
    CONSTRAINT ck_loyalty_policy_target CHECK (target_stamp_count BETWEEN 2 AND 100),
    CONSTRAINT ck_loyalty_policy_per_order CHECK (stamps_per_order BETWEEN 1 AND 10),
    CONSTRAINT ck_loyalty_policy_min_amount CHECK (minimum_order_amount BETWEEN 0 AND 100000000),
    CONSTRAINT ck_loyalty_policy_daily_limit CHECK (daily_earning_limit IS NULL OR daily_earning_limit BETWEEN 1 AND 20),
    CONSTRAINT ck_loyalty_policy_duplicate_window CHECK (duplicate_warning_seconds BETWEEN 60 AND 3600),
    CONSTRAINT ck_loyalty_policy_stamp_validity CHECK (stamp_validity_days BETWEEN 1 AND 730),
    CONSTRAINT ck_loyalty_policy_schedule CHECK (ends_at IS NULL OR starts_at IS NULL OR ends_at > starts_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_loyalty_policies_active_store
    ON coupon.loyalty_policies (store_id)
    WHERE status = 'ACTIVE';
CREATE INDEX IF NOT EXISTS ix_loyalty_policies_store_status
    ON coupon.loyalty_policies (store_id, status, starts_at);

CREATE TABLE IF NOT EXISTS coupon.loyalty_reward_definitions (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    policy_id uuid NOT NULL UNIQUE REFERENCES coupon.loyalty_policies(id) ON DELETE RESTRICT,
    benefit_type coupon.benefit_type NOT NULL,
    fixed_amount bigint,
    percentage smallint,
    maximum_discount_amount bigint,
    free_item_ids uuid[] NOT NULL DEFAULT '{}',
    minimum_order_amount bigint NOT NULL DEFAULT 0,
    validity_days smallint NOT NULL DEFAULT 30,
    condition_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    schema_version integer NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_reward_min_amount CHECK (minimum_order_amount >= 0),
    CONSTRAINT ck_reward_validity CHECK (validity_days BETWEEN 1 AND 365),
    CONSTRAINT ck_reward_benefit_fields CHECK (
        (benefit_type = 'FIXED_AMOUNT' AND fixed_amount > 0 AND percentage IS NULL AND cardinality(free_item_ids) = 0)
        OR (benefit_type = 'PERCENTAGE' AND percentage BETWEEN 1 AND 100 AND maximum_discount_amount > 0 AND fixed_amount IS NULL AND cardinality(free_item_ids) = 0)
        OR (benefit_type = 'FREE_ITEM' AND cardinality(free_item_ids) > 0 AND fixed_amount IS NULL AND percentage IS NULL)
    )
);

CREATE TABLE IF NOT EXISTS coupon.campaigns (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    version_no integer NOT NULL DEFAULT 1,
    status coupon.campaign_status NOT NULL DEFAULT 'DRAFT',
    name varchar(160) NOT NULL,
    customer_description text NOT NULL,
    benefit_type coupon.benefit_type NOT NULL,
    fixed_amount bigint,
    percentage smallint,
    maximum_discount_amount bigint,
    free_item_ids uuid[] NOT NULL DEFAULT '{}',
    minimum_order_amount bigint NOT NULL DEFAULT 0,
    eligible_item_ids uuid[] NOT NULL DEFAULT '{}',
    eligible_category_ids uuid[] NOT NULL DEFAULT '{}',
    excluded_item_ids uuid[] NOT NULL DEFAULT '{}',
    issue_mode coupon.issue_mode NOT NULL,
    audience_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    total_quantity bigint,
    per_user_quantity smallint NOT NULL DEFAULT 1,
    per_business_day_quantity bigint,
    global_reserved_count bigint NOT NULL DEFAULT 0,
    global_issued_count bigint NOT NULL DEFAULT 0,
    global_revoked_count bigint NOT NULL DEFAULT 0,
    restore_quantity_on_revoke boolean NOT NULL DEFAULT false,
    issue_starts_at timestamptz NOT NULL,
    issue_ends_at timestamptz NOT NULL,
    usable_from timestamptz,
    usable_until timestamptz,
    relative_validity_days smallint,
    allowed_weekdays smallint[] NOT NULL DEFAULT '{}',
    allowed_local_time_ranges jsonb NOT NULL DEFAULT '[]'::jsonb,
    external_discount_stackable boolean NOT NULL DEFAULT false,
    condition_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    schema_version integer NOT NULL DEFAULT 1,
    published_at timestamptz,
    paused_at timestamptz,
    ended_at timestamptz,
    cancelled_at timestamptz,
    created_by_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_campaigns_store_version UNIQUE (store_id, id, version_no),
    CONSTRAINT ck_campaign_name CHECK (btrim(name) <> ''),
    CONSTRAINT ck_campaign_description CHECK (btrim(customer_description) <> ''),
    CONSTRAINT ck_campaign_min_amount CHECK (minimum_order_amount >= 0),
    CONSTRAINT ck_campaign_total_quantity CHECK (total_quantity IS NULL OR total_quantity BETWEEN 1 AND 1000000),
    CONSTRAINT ck_campaign_per_user_quantity CHECK (per_user_quantity BETWEEN 1 AND 100),
    CONSTRAINT ck_campaign_daily_quantity CHECK (per_business_day_quantity IS NULL OR per_business_day_quantity > 0),
    CONSTRAINT ck_campaign_global_counts CHECK (
        global_reserved_count >= 0 AND global_issued_count >= 0 AND global_revoked_count >= 0
        AND (total_quantity IS NULL OR global_reserved_count + global_issued_count <= total_quantity)
    ),
    CONSTRAINT ck_campaign_issue_schedule CHECK (issue_ends_at > issue_starts_at),
    CONSTRAINT ck_campaign_usage_schedule CHECK (usable_until IS NULL OR usable_until > COALESCE(usable_from, issue_starts_at)),
    CONSTRAINT ck_campaign_relative_validity CHECK (relative_validity_days IS NULL OR relative_validity_days BETWEEN 1 AND 365),
    CONSTRAINT ck_campaign_validity_mode CHECK (usable_until IS NOT NULL OR relative_validity_days IS NOT NULL),
    CONSTRAINT ck_campaign_benefit_fields CHECK (
        (benefit_type = 'FIXED_AMOUNT' AND fixed_amount > 0 AND percentage IS NULL AND cardinality(free_item_ids) = 0)
        OR (benefit_type = 'PERCENTAGE' AND percentage BETWEEN 1 AND 100 AND maximum_discount_amount > 0 AND fixed_amount IS NULL AND cardinality(free_item_ids) = 0)
        OR (benefit_type = 'FREE_ITEM' AND cardinality(free_item_ids) > 0 AND fixed_amount IS NULL AND percentage IS NULL)
    ),
    CONSTRAINT ck_campaign_allowed_weekdays CHECK (allowed_weekdays <@ ARRAY[0,1,2,3,4,5,6]::smallint[])
);

-- Kept outside CREATE TABLE as well so a safe re-run upgrades a database that
-- received an earlier draft of this initial migration.
ALTER TABLE coupon.campaigns
    ADD COLUMN IF NOT EXISTS global_reserved_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS global_issued_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS global_revoked_count bigint NOT NULL DEFAULT 0;
ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_global_counts;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_global_counts CHECK (
        global_reserved_count >= 0 AND global_issued_count >= 0 AND global_revoked_count >= 0
        AND (total_quantity IS NULL OR global_reserved_count + global_issued_count <= total_quantity)
    );

CREATE INDEX IF NOT EXISTS ix_campaigns_store_status
    ON coupon.campaigns (store_id, status, issue_starts_at DESC);
CREATE INDEX IF NOT EXISTS ix_campaigns_public_claim
    ON coupon.campaigns (issue_starts_at, issue_ends_at)
    WHERE issue_mode = 'FIRST_COME' AND status IN ('SCHEDULED', 'ISSUING');

CREATE TABLE IF NOT EXISTS coupon.campaign_audience_members (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    campaign_id uuid NOT NULL REFERENCES coupon.campaigns(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    snapshot_reason jsonb NOT NULL DEFAULT '{}'::jsonb,
    status varchar(32) NOT NULL DEFAULT 'PENDING',
    processed_at timestamptz,
    coupon_id uuid,
    error_code varchar(100),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_campaign_audience_member UNIQUE (campaign_id, user_id),
    CONSTRAINT ck_campaign_audience_status CHECK (status IN ('PENDING', 'ISSUED', 'SKIPPED', 'FAILED'))
);

CREATE INDEX IF NOT EXISTS ix_campaign_audience_work
    ON coupon.campaign_audience_members (campaign_id, status, user_id);

CREATE TABLE IF NOT EXISTS coupon.campaign_counters (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    campaign_id uuid NOT NULL REFERENCES coupon.campaigns(id) ON DELETE RESTRICT,
    business_day date NOT NULL,
    reserved_count bigint NOT NULL DEFAULT 0,
    issued_count bigint NOT NULL DEFAULT 0,
    revoked_count bigint NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_campaign_counters_day UNIQUE (campaign_id, business_day),
    CONSTRAINT ck_campaign_counter_nonnegative CHECK (
        reserved_count >= 0 AND issued_count >= 0 AND revoked_count >= 0
    )
);

CREATE TABLE IF NOT EXISTS coupon.coupon_instances (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    source_type coupon.coupon_source_type NOT NULL,
    campaign_id uuid REFERENCES coupon.campaigns(id) ON DELETE RESTRICT,
    reward_definition_id uuid REFERENCES coupon.loyalty_reward_definitions(id) ON DELETE RESTRICT,
    admin_case_id uuid,
    issuance_ordinal smallint NOT NULL DEFAULT 1,
    status coupon.coupon_status NOT NULL,
    title varchar(200) NOT NULL,
    description text NOT NULL,
    benefit_type coupon.benefit_type NOT NULL,
    usable_from timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    condition_snapshot jsonb NOT NULL,
    schema_version integer NOT NULL DEFAULT 1,
    issued_at timestamptz,
    reserved_at timestamptz,
    used_at timestamptz,
    expired_at timestamptz,
    revoked_at timestamptz,
    voided_at timestamptz,
    revocation_reason text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_coupon_instance_period CHECK (expires_at > usable_from),
    CONSTRAINT ck_coupon_instance_ordinal CHECK (issuance_ordinal >= 1),
    CONSTRAINT ck_coupon_instance_source CHECK (
        (source_type = 'CAMPAIGN' AND campaign_id IS NOT NULL AND reward_definition_id IS NULL)
        OR (source_type = 'LOYALTY_REWARD' AND campaign_id IS NULL AND reward_definition_id IS NOT NULL)
        OR (source_type = 'ADMIN_GRANT' AND campaign_id IS NULL AND reward_definition_id IS NULL AND admin_case_id IS NOT NULL)
    ),
    CONSTRAINT ck_coupon_instance_terminal_timestamp CHECK (
        (status <> 'USED' OR used_at IS NOT NULL)
        AND (status <> 'EXPIRED' OR expired_at IS NOT NULL)
        AND (status <> 'REVOKED' OR revoked_at IS NOT NULL)
        AND (status <> 'VOIDED' OR voided_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_coupon_campaign_user_ordinal
    ON coupon.coupon_instances (campaign_id, user_id, issuance_ordinal)
    WHERE campaign_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_coupon_wallet_status_expiry
    ON coupon.coupon_instances (user_id, status, expires_at);
CREATE INDEX IF NOT EXISTS ix_coupon_store_status
    ON coupon.coupon_instances (store_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS ix_coupon_campaign_status
    ON coupon.coupon_instances (campaign_id, status)
    WHERE campaign_id IS NOT NULL;

ALTER TABLE coupon.campaign_audience_members
    DROP CONSTRAINT IF EXISTS fk_campaign_audience_coupon;
ALTER TABLE coupon.campaign_audience_members
    ADD CONSTRAINT fk_campaign_audience_coupon
    FOREIGN KEY (coupon_id) REFERENCES coupon.coupon_instances(id) ON DELETE RESTRICT;

CREATE TABLE IF NOT EXISTS coupon.coupon_status_events (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    coupon_id uuid NOT NULL REFERENCES coupon.coupon_instances(id) ON DELETE RESTRICT,
    from_status coupon.coupon_status,
    to_status coupon.coupon_status NOT NULL,
    actor_type coupon.actor_type NOT NULL,
    actor_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    reason_code varchar(100) NOT NULL,
    reason_detail text,
    transaction_id uuid,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_coupon_status_changed CHECK (from_status IS NULL OR from_status <> to_status)
);

CREATE INDEX IF NOT EXISTS ix_coupon_status_events_coupon_time
    ON coupon.coupon_status_events (coupon_id, occurred_at, id);

CREATE TABLE IF NOT EXISTS coupon.issuance_deduplications (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    campaign_id uuid NOT NULL REFERENCES coupon.campaigns(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    ordinal smallint NOT NULL,
    coupon_id uuid NOT NULL REFERENCES coupon.coupon_instances(id) ON DELETE RESTRICT,
    idempotency_key uuid,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_issuance_dedup_campaign_user_ordinal UNIQUE (campaign_id, user_id, ordinal),
    CONSTRAINT uq_issuance_dedup_coupon UNIQUE (coupon_id),
    CONSTRAINT ck_issuance_dedup_ordinal CHECK (ordinal >= 1)
);

CREATE TABLE IF NOT EXISTS coupon.stamp_transactions (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    policy_id uuid NOT NULL REFERENCES coupon.loyalty_policies(id) ON DELETE RESTRICT,
    business_day date NOT NULL,
    quantity smallint NOT NULL,
    external_order_ref varchar(200),
    order_snapshot jsonb NOT NULL,
    status coupon.stamp_transaction_status NOT NULL DEFAULT 'CONFIRMED',
    approved_by_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    voided_at timestamptz,
    voided_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    void_reason text,
    idempotency_key uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_stamp_transaction_idempotency UNIQUE (approved_by_user_id, idempotency_key),
    CONSTRAINT ck_stamp_transaction_quantity CHECK (quantity BETWEEN 1 AND 10),
    CONSTRAINT ck_stamp_transaction_void CHECK (
        (status = 'CONFIRMED' AND voided_at IS NULL)
        OR (status <> 'CONFIRMED' AND voided_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS ix_stamp_transactions_daily_limit
    ON coupon.stamp_transactions (store_id, user_id, policy_id, business_day, status);
CREATE INDEX IF NOT EXISTS ix_stamp_transactions_order_ref
    ON coupon.stamp_transactions (store_id, external_order_ref)
    WHERE external_order_ref IS NOT NULL;

CREATE TABLE IF NOT EXISTS coupon.stamp_lots (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    policy_id uuid NOT NULL REFERENCES coupon.loyalty_policies(id) ON DELETE RESTRICT,
    source_transaction_id uuid NOT NULL REFERENCES coupon.stamp_transactions(id) ON DELETE RESTRICT,
    original_quantity smallint NOT NULL,
    earned_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_stamp_lot_source UNIQUE (source_transaction_id),
    CONSTRAINT ck_stamp_lot_quantity CHECK (original_quantity BETWEEN 1 AND 10),
    CONSTRAINT ck_stamp_lot_period CHECK (expires_at > earned_at)
);

CREATE INDEX IF NOT EXISTS ix_stamp_lots_wallet_expiry
    ON coupon.stamp_lots (user_id, store_id, expires_at);

CREATE TABLE IF NOT EXISTS coupon.stamp_ledger (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    lot_id uuid NOT NULL REFERENCES coupon.stamp_lots(id) ON DELETE RESTRICT,
    event_type coupon.stamp_ledger_event_type NOT NULL,
    quantity_delta smallint NOT NULL,
    source_stamp_transaction_id uuid REFERENCES coupon.stamp_transactions(id) ON DELETE RESTRICT,
    reward_coupon_id uuid REFERENCES coupon.coupon_instances(id) ON DELETE RESTRICT,
    admin_case_id uuid,
    actor_type coupon.actor_type NOT NULL,
    actor_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    reason_code varchar(100) NOT NULL,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_stamp_ledger_nonzero CHECK (quantity_delta <> 0),
    CONSTRAINT ck_stamp_ledger_direction CHECK (
        (event_type = 'EARN' AND quantity_delta > 0)
        OR (event_type IN ('CONSUME_FOR_REWARD', 'EXPIRE') AND quantity_delta < 0)
        OR event_type IN ('VOID', 'ADMIN_ADJUSTMENT')
    ),
    CONSTRAINT ck_stamp_ledger_reward_link CHECK (
        event_type <> 'CONSUME_FOR_REWARD' OR reward_coupon_id IS NOT NULL
    )
);

ALTER TABLE coupon.stamp_ledger
    DROP CONSTRAINT IF EXISTS ck_stamp_ledger_direction;
ALTER TABLE coupon.stamp_ledger
    ADD CONSTRAINT ck_stamp_ledger_direction CHECK (
        (event_type = 'EARN' AND quantity_delta > 0)
        OR (event_type IN ('CONSUME_FOR_REWARD', 'EXPIRE') AND quantity_delta < 0)
        OR event_type IN ('VOID', 'ADMIN_ADJUSTMENT')
    );

CREATE INDEX IF NOT EXISTS ix_stamp_ledger_lot_time
    ON coupon.stamp_ledger (lot_id, occurred_at, id);
CREATE INDEX IF NOT EXISTS ix_stamp_ledger_reward_coupon
    ON coupon.stamp_ledger (reward_coupon_id)
    WHERE reward_coupon_id IS NOT NULL;

CREATE OR REPLACE FUNCTION coupon.enforce_stamp_lot_balance()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    lot_capacity smallint;
    current_balance bigint;
BEGIN
    SELECT original_quantity
    INTO lot_capacity
    FROM coupon.stamp_lots
    WHERE id = NEW.lot_id
    FOR UPDATE;

    SELECT COALESCE(SUM(quantity_delta), 0)
    INTO current_balance
    FROM coupon.stamp_ledger
    WHERE lot_id = NEW.lot_id;

    IF current_balance < 0 OR current_balance > lot_capacity THEN
        RAISE EXCEPTION 'stamp lot % balance % is outside 0..%', NEW.lot_id, current_balance, lot_capacity
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_stamp_ledger_balance ON coupon.stamp_ledger;
CREATE CONSTRAINT TRIGGER trg_stamp_ledger_balance
AFTER INSERT ON coupon.stamp_ledger
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION coupon.enforce_stamp_lot_balance();

CREATE TABLE IF NOT EXISTS coupon.qr_nonces (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    nonce_hash bytea NOT NULL UNIQUE,
    fallback_code_hash bytea NOT NULL UNIQUE,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    audience varchar(100) NOT NULL,
    key_id varchar(64) NOT NULL,
    issued_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    consumed_transaction_type varchar(64),
    consumed_transaction_id uuid,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_qr_nonce_period CHECK (expires_at > issued_at),
    CONSTRAINT ck_qr_nonce_consumption CHECK (
        (consumed_at IS NULL AND consumed_transaction_id IS NULL)
        OR (consumed_at IS NOT NULL AND consumed_transaction_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS ix_qr_nonces_user_expiry
    ON coupon.qr_nonces (user_id, expires_at DESC);
CREATE INDEX IF NOT EXISTS ix_qr_nonces_expiry_cleanup
    ON coupon.qr_nonces (expires_at)
    WHERE consumed_at IS NULL AND revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS coupon.redemption_reservations (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    coupon_id uuid NOT NULL REFERENCES coupon.coupon_instances(id) ON DELETE RESTRICT,
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    owner_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    owner_session_id varchar(255) NOT NULL,
    qr_nonce_id uuid NOT NULL REFERENCES coupon.qr_nonces(id) ON DELETE RESTRICT,
    status coupon.reservation_status NOT NULL DEFAULT 'ACTIVE',
    order_snapshot jsonb NOT NULL,
    order_snapshot_hash char(64) NOT NULL,
    expected_discount_amount bigint NOT NULL,
    reserved_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    completed_at timestamptz,
    idempotency_key uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_redemption_reservation_idempotency UNIQUE (owner_user_id, idempotency_key),
    CONSTRAINT ck_redemption_reservation_discount CHECK (expected_discount_amount >= 0),
    CONSTRAINT ck_redemption_reservation_period CHECK (expires_at > reserved_at),
    CONSTRAINT ck_redemption_reservation_completed CHECK (
        (status = 'ACTIVE' AND completed_at IS NULL)
        OR (status <> 'ACTIVE' AND completed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_redemption_reservations_active_coupon
    ON coupon.redemption_reservations (coupon_id)
    WHERE status = 'ACTIVE';
CREATE UNIQUE INDEX IF NOT EXISTS uq_redemption_reservations_active_owner_session
    ON coupon.redemption_reservations (owner_session_id)
    WHERE status = 'ACTIVE';
CREATE INDEX IF NOT EXISTS ix_redemption_reservations_expiry
    ON coupon.redemption_reservations (expires_at)
    WHERE status = 'ACTIVE';

CREATE TABLE IF NOT EXISTS coupon.redemption_transactions (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    coupon_id uuid NOT NULL REFERENCES coupon.coupon_instances(id) ON DELETE RESTRICT,
    reservation_id uuid NOT NULL UNIQUE REFERENCES coupon.redemption_reservations(id) ON DELETE RESTRICT,
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    approved_by_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    external_order_ref varchar(200),
    order_snapshot jsonb NOT NULL,
    gross_amount bigint NOT NULL,
    eligible_amount bigint NOT NULL,
    discount_amount bigint NOT NULL,
    currency char(3) NOT NULL DEFAULT 'KRW',
    status coupon.redemption_status NOT NULL DEFAULT 'CONFIRMED',
    confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    voided_at timestamptz,
    voided_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    void_reason text,
    idempotency_key uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_redemption_transaction_idempotency UNIQUE (approved_by_user_id, idempotency_key),
    CONSTRAINT ck_redemption_amounts CHECK (
        gross_amount >= 0
        AND eligible_amount BETWEEN 0 AND gross_amount
        AND discount_amount BETWEEN 0 AND eligible_amount
    ),
    CONSTRAINT ck_redemption_currency CHECK (currency = 'KRW'),
    CONSTRAINT ck_redemption_void CHECK (
        (status = 'CONFIRMED' AND voided_at IS NULL)
        OR (status <> 'CONFIRMED' AND voided_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_redemption_transactions_confirmed_coupon
    ON coupon.redemption_transactions (coupon_id)
    WHERE status = 'CONFIRMED';
CREATE INDEX IF NOT EXISTS ix_redemption_transactions_store_time
    ON coupon.redemption_transactions (store_id, confirmed_at DESC);

CREATE TABLE IF NOT EXISTS coupon.notifications (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    store_id uuid REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    event_id uuid NOT NULL,
    notification_type varchar(100) NOT NULL,
    purpose varchar(64) NOT NULL,
    title varchar(200) NOT NULL,
    body text NOT NULL,
    deep_link text,
    data jsonb NOT NULL DEFAULT '{}'::jsonb,
    occurred_at timestamptz NOT NULL,
    read_at timestamptz,
    expires_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_notifications_user_event_type UNIQUE (user_id, event_id, notification_type),
    CONSTRAINT ck_notifications_title CHECK (btrim(title) <> ''),
    CONSTRAINT ck_notifications_body CHECK (btrim(body) <> '')
);

CREATE INDEX IF NOT EXISTS ix_notifications_inbox
    ON coupon.notifications (user_id, read_at, occurred_at DESC);

CREATE TABLE IF NOT EXISTS coupon.notification_templates (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    code varchar(100) NOT NULL,
    version_no integer NOT NULL,
    locale varchar(16) NOT NULL DEFAULT 'ko-KR',
    channel coupon.notification_channel NOT NULL,
    purpose varchar(64) NOT NULL,
    provider_template_id varchar(200),
    provider_approval_status varchar(32),
    subject_template text,
    body_template text NOT NULL,
    variable_schema jsonb NOT NULL DEFAULT '{}'::jsonb,
    active boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_notification_template UNIQUE (code, version_no, locale, channel),
    CONSTRAINT ck_notification_template_version CHECK (version_no >= 1)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_notification_template_active
    ON coupon.notification_templates (code, locale, channel)
    WHERE active;

CREATE TABLE IF NOT EXISTS coupon.notification_deliveries (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    notification_id uuid NOT NULL REFERENCES coupon.notifications(id) ON DELETE RESTRICT,
    channel coupon.notification_channel NOT NULL,
    template_id uuid REFERENCES coupon.notification_templates(id) ON DELETE RESTRICT,
    recipient_reference_hash bytea,
    status coupon.notification_delivery_status NOT NULL DEFAULT 'PENDING',
    attempt_count integer NOT NULL DEFAULT 0,
    next_attempt_at timestamptz,
    provider_reference varchar(255),
    provider_status varchar(100),
    last_error_code varchar(100),
    last_error_message text,
    sent_at timestamptz,
    delivered_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_notification_delivery_channel UNIQUE (notification_id, channel),
    CONSTRAINT ck_notification_delivery_attempts CHECK (attempt_count >= 0)
);

CREATE INDEX IF NOT EXISTS ix_notification_deliveries_work
    ON coupon.notification_deliveries (status, next_attempt_at)
    WHERE status IN ('PENDING', 'FAILED_RETRYABLE');
CREATE UNIQUE INDEX IF NOT EXISTS uq_notification_deliveries_provider_ref
    ON coupon.notification_deliveries (channel, provider_reference)
    WHERE provider_reference IS NOT NULL;

CREATE TABLE IF NOT EXISTS coupon.push_subscriptions (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    token_ciphertext bytea NOT NULL,
    token_lookup_hash bytea NOT NULL UNIQUE,
    browser_family varchar(64),
    device_label varchar(100),
    status coupon.resource_status NOT NULL DEFAULT 'ACTIVE',
    last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    failure_count integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_push_subscription_failures CHECK (failure_count >= 0)
);

CREATE INDEX IF NOT EXISTS ix_push_subscriptions_user_status
    ON coupon.push_subscriptions (user_id, status);

CREATE TABLE IF NOT EXISTS coupon.idempotency_requests (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    actor_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    operation varchar(120) NOT NULL,
    idempotency_key uuid NOT NULL,
    request_hash char(64) NOT NULL,
    status varchar(20) NOT NULL DEFAULT 'PROCESSING',
    response_status integer,
    response_body jsonb,
    resource_id uuid,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    completed_at timestamptz,
    expires_at timestamptz NOT NULL,
    CONSTRAINT uq_idempotency_request UNIQUE (actor_user_id, operation, idempotency_key),
    CONSTRAINT ck_idempotency_status CHECK (status IN ('PROCESSING', 'COMPLETED', 'FAILED')),
    CONSTRAINT ck_idempotency_response CHECK (
        (status = 'PROCESSING' AND completed_at IS NULL)
        OR (status <> 'PROCESSING' AND completed_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS ix_idempotency_requests_expiry
    ON coupon.idempotency_requests (expires_at);

CREATE TABLE IF NOT EXISTS coupon.outbox_events (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    aggregate_type varchar(100) NOT NULL,
    aggregate_id uuid NOT NULL,
    aggregate_version bigint NOT NULL,
    event_type varchar(120) NOT NULL,
    correlation_id uuid NOT NULL,
    payload jsonb NOT NULL,
    status coupon.outbox_status NOT NULL DEFAULT 'PENDING',
    attempt_count integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    locked_by varchar(200),
    locked_at timestamptz,
    published_at timestamptz,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_outbox_aggregate_event UNIQUE (aggregate_type, aggregate_id, aggregate_version, event_type),
    CONSTRAINT ck_outbox_attempts CHECK (attempt_count >= 0)
);

CREATE INDEX IF NOT EXISTS ix_outbox_events_pending
    ON coupon.outbox_events (available_at, created_at)
    WHERE status IN ('PENDING', 'FAILED');

CREATE TABLE IF NOT EXISTS coupon.job_registry (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    unique_key varchar(500) NOT NULL,
    job_type varchar(120) NOT NULL,
    generation integer NOT NULL DEFAULT 1,
    status coupon.job_status NOT NULL DEFAULT 'PENDING_OUTBOX',
    payload jsonb NOT NULL,
    checkpoint jsonb NOT NULL DEFAULT '{}'::jsonb,
    processed_count bigint NOT NULL DEFAULT 0,
    succeeded_count bigint NOT NULL DEFAULT 0,
    failed_count bigint NOT NULL DEFAULT 0,
    attempt_count integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL DEFAULT 5,
    scheduled_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    next_attempt_at timestamptz,
    started_at timestamptz,
    heartbeat_at timestamptz,
    finished_at timestamptz,
    locked_by varchar(200),
    last_error_code varchar(100),
    last_error_message text,
    requested_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_job_registry_generation UNIQUE (unique_key, generation),
    CONSTRAINT ck_job_generation CHECK (generation >= 1),
    CONSTRAINT ck_job_counts CHECK (
        processed_count >= 0 AND succeeded_count >= 0 AND failed_count >= 0
        AND processed_count >= succeeded_count + failed_count
    ),
    CONSTRAINT ck_job_attempts CHECK (attempt_count >= 0 AND max_attempts >= 1)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_job_registry_active_key
    ON coupon.job_registry (unique_key)
    WHERE status IN ('PENDING_OUTBOX', 'QUEUED', 'RUNNING', 'RETRY_WAIT', 'PAUSE_REQUESTED', 'PAUSED');
CREATE INDEX IF NOT EXISTS ix_job_registry_work
    ON coupon.job_registry (status, COALESCE(next_attempt_at, scheduled_at));
CREATE INDEX IF NOT EXISTS ix_job_registry_heartbeat
    ON coupon.job_registry (heartbeat_at)
    WHERE status = 'RUNNING';

CREATE TABLE IF NOT EXISTS coupon.admin_cases (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    case_number bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    case_type coupon.admin_case_type NOT NULL,
    status coupon.admin_case_status NOT NULL DEFAULT 'OPEN',
    priority smallint NOT NULL DEFAULT 3,
    title varchar(200) NOT NULL,
    description text NOT NULL,
    subject_user_id uuid REFERENCES coupon.users(id) ON DELETE RESTRICT,
    subject_store_id uuid REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    subject_resource_type varchar(100),
    subject_resource_id uuid,
    assignee_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    opened_by_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    public_resolution text,
    internal_resolution text,
    legal_hold_until timestamptz,
    resolved_at timestamptz,
    closed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_admin_case_priority CHECK (priority BETWEEN 1 AND 5),
    CONSTRAINT ck_admin_case_title CHECK (btrim(title) <> ''),
    CONSTRAINT ck_admin_case_resolution CHECK (
        (status NOT IN ('RESOLVED', 'CLOSED') OR resolved_at IS NOT NULL)
        AND (status <> 'CLOSED' OR closed_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS ix_admin_cases_queue
    ON coupon.admin_cases (status, priority, created_at);
CREATE INDEX IF NOT EXISTS ix_admin_cases_user
    ON coupon.admin_cases (subject_user_id, created_at DESC)
    WHERE subject_user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_admin_cases_store
    ON coupon.admin_cases (subject_store_id, created_at DESC)
    WHERE subject_store_id IS NOT NULL;

ALTER TABLE coupon.coupon_instances
    DROP CONSTRAINT IF EXISTS fk_coupon_instances_admin_case;
ALTER TABLE coupon.coupon_instances
    ADD CONSTRAINT fk_coupon_instances_admin_case
    FOREIGN KEY (admin_case_id) REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT;

ALTER TABLE coupon.stamp_ledger
    DROP CONSTRAINT IF EXISTS fk_stamp_ledger_admin_case;
ALTER TABLE coupon.stamp_ledger
    ADD CONSTRAINT fk_stamp_ledger_admin_case
    FOREIGN KEY (admin_case_id) REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT;

CREATE TABLE IF NOT EXISTS coupon.admin_case_notes (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    case_id uuid NOT NULL REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT,
    author_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    visibility varchar(20) NOT NULL DEFAULT 'INTERNAL',
    body text NOT NULL,
    attachment_refs jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_admin_case_note_visibility CHECK (visibility IN ('INTERNAL', 'CUSTOMER', 'STORE')),
    CONSTRAINT ck_admin_case_note_body CHECK (btrim(body) <> '')
);

CREATE INDEX IF NOT EXISTS ix_admin_case_notes_case_time
    ON coupon.admin_case_notes (case_id, created_at);

CREATE TABLE IF NOT EXISTS coupon.admin_adjustments (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    case_id uuid NOT NULL REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT,
    status coupon.admin_adjustment_status NOT NULL DEFAULT 'DRAFT',
    adjustment_type varchar(100) NOT NULL,
    target_snapshot jsonb NOT NULL,
    preview_snapshot jsonb NOT NULL,
    preview_expires_at timestamptz NOT NULL,
    requested_by_user_id uuid NOT NULL REFERENCES coupon.users(id) ON DELETE RESTRICT,
    requested_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    approved_by_user_id uuid REFERENCES coupon.users(id) ON DELETE RESTRICT,
    approved_at timestamptz,
    execution_job_id uuid REFERENCES coupon.job_registry(id) ON DELETE RESTRICT,
    executed_at timestamptz,
    failure_reason text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT ck_admin_adjustment_preview_period CHECK (preview_expires_at > requested_at),
    CONSTRAINT ck_admin_adjustment_separation CHECK (
        approved_by_user_id IS NULL OR approved_by_user_id <> requested_by_user_id
    ),
    CONSTRAINT ck_admin_adjustment_approval CHECK (
        status NOT IN ('APPROVED', 'EXECUTING', 'SUCCEEDED')
        OR (approved_by_user_id IS NOT NULL AND approved_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS ix_admin_adjustments_case
    ON coupon.admin_adjustments (case_id, created_at DESC);

CREATE TABLE IF NOT EXISTS coupon.audit_logs (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    actor_type coupon.actor_type NOT NULL,
    actor_user_id uuid REFERENCES coupon.users(id) ON DELETE SET NULL,
    actor_session_id varchar(255),
    action varchar(160) NOT NULL,
    resource_type varchar(120) NOT NULL,
    resource_id uuid,
    store_id uuid REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    case_id uuid REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT,
    reason text,
    request_id varchar(100),
    correlation_id uuid,
    source_ip_hash bytea,
    user_agent_class varchar(64),
    before_hash char(64),
    after_hash char(64),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    previous_entry_hash char(64),
    entry_hash char(64),
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_audit_action CHECK (btrim(action) <> ''),
    CONSTRAINT ck_audit_resource_type CHECK (btrim(resource_type) <> '')
);

CREATE INDEX IF NOT EXISTS ix_audit_logs_resource
    ON coupon.audit_logs (resource_type, resource_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS ix_audit_logs_actor
    ON coupon.audit_logs (actor_user_id, occurred_at DESC)
    WHERE actor_user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_audit_logs_store
    ON coupon.audit_logs (store_id, occurred_at DESC)
    WHERE store_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS coupon.analytics_daily_store (
    store_id uuid NOT NULL REFERENCES coupon.stores(id) ON DELETE RESTRICT,
    business_day date NOT NULL,
    stamp_earned_count bigint NOT NULL DEFAULT 0,
    stamp_voided_count bigint NOT NULL DEFAULT 0,
    active_customer_count bigint NOT NULL DEFAULT 0,
    reward_issued_count bigint NOT NULL DEFAULT 0,
    reward_used_count bigint NOT NULL DEFAULT 0,
    campaign_coupon_issued_count bigint NOT NULL DEFAULT 0,
    campaign_coupon_used_count bigint NOT NULL DEFAULT 0,
    coupon_expired_count bigint NOT NULL DEFAULT 0,
    computed_through timestamptz NOT NULL,
    is_final boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    PRIMARY KEY (store_id, business_day),
    CONSTRAINT ck_analytics_counts_nonnegative CHECK (
        stamp_earned_count >= 0 AND stamp_voided_count >= 0 AND active_customer_count >= 0
        AND reward_issued_count >= 0 AND reward_used_count >= 0
        AND campaign_coupon_issued_count >= 0 AND campaign_coupon_used_count >= 0
        AND coupon_expired_count >= 0
    )
);

CREATE TABLE IF NOT EXISTS coupon.deletion_ledger (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    subject_user_id uuid NOT NULL,
    case_id uuid REFERENCES coupon.admin_cases(id) ON DELETE RESTRICT,
    requested_at timestamptz NOT NULL,
    execute_after timestamptz NOT NULL,
    executed_at timestamptz,
    status varchar(32) NOT NULL DEFAULT 'PENDING',
    deletion_scope jsonb NOT NULL,
    backup_tombstone_version bigint NOT NULL DEFAULT 1,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    version bigint NOT NULL DEFAULT 1,
    CONSTRAINT uq_deletion_ledger_active_subject UNIQUE (subject_user_id, backup_tombstone_version),
    CONSTRAINT ck_deletion_ledger_period CHECK (execute_after >= requested_at),
    CONSTRAINT ck_deletion_ledger_status CHECK (status IN ('PENDING', 'BLOCKED_LEGAL_HOLD', 'RUNNING', 'SUCCEEDED', 'FAILED'))
);

CREATE INDEX IF NOT EXISTS ix_deletion_ledger_work
    ON coupon.deletion_ledger (status, execute_after)
    WHERE status IN ('PENDING', 'FAILED');

-- Mutable aggregate tables use optimistic versions and server-side updated timestamps.
DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'users', 'auth_identities', 'stores', 'store_business_profiles', 'store_reviews',
        'store_customers', 'notification_preferences', 'catalog_categories', 'catalog_items',
        'loyalty_policies', 'campaigns', 'campaign_audience_members', 'campaign_counters',
        'coupon_instances', 'stamp_transactions', 'redemption_reservations',
        'redemption_transactions', 'notification_deliveries', 'push_subscriptions',
        'job_registry', 'admin_cases', 'admin_adjustments', 'analytics_daily_store',
        'deletion_ledger'
    ]
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

-- These records are evidence. Corrections must be represented by new events.
DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'consent_events', 'coupon_status_events', 'stamp_ledger', 'audit_logs'
    ]
    LOOP
        EXECUTE format('DROP TRIGGER IF EXISTS trg_%I_append_only ON coupon.%I', table_name, table_name);
        EXECUTE format(
            'CREATE TRIGGER trg_%I_append_only BEFORE UPDATE OR DELETE ON coupon.%I '
            'FOR EACH ROW EXECUTE FUNCTION coupon.reject_append_only_mutation()',
            table_name, table_name
        );
    END LOOP;
END;
$$;

COMMENT ON TABLE coupon.stamp_ledger IS 'Append-only stamp balance ledger; available balance is the sum of quantity_delta per lot';
COMMENT ON TABLE coupon.coupon_status_events IS 'Append-only coupon state transition history';
COMMENT ON TABLE coupon.outbox_events IS 'Transactional outbox used to recover Redis enqueue failures';
COMMENT ON TABLE coupon.job_registry IS 'Canonical job lifecycle and singleton execution registry; Redis is transport only';
COMMENT ON TABLE coupon.audit_logs IS 'Append-only security and administration audit trail';
COMMENT ON COLUMN coupon.users.consumer_key IS 'Permanent internal consumer identifier; never expose it directly in QR payloads';
COMMENT ON COLUMN coupon.qr_nonces.nonce_hash IS 'Hash of a short-lived random nonce; plaintext is never persisted';

INSERT INTO coupon.schema_metadata (key, value)
VALUES
    ('schema_version', '20260810000100'),
    ('default_locale', 'ko-KR'),
    ('default_currency', 'KRW'),
    ('default_timezone', 'Asia/Seoul')
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    updated_at = clock_timestamp();
