-- Bank profile layer on top of passkey-rs users (auth DB).
CREATE TABLE IF NOT EXISTS bank_profiles (
    user_id UUID PRIMARY KEY,
    handle TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT bank_profiles_handle_format CHECK (
        handle ~ '^[a-z][a-z0-9_]{2,31}$'
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS bank_profiles_handle_uidx
    ON bank_profiles (handle);
