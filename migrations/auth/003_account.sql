-- Account center: auth event log + personal data update requests.

CREATE TABLE IF NOT EXISTS auth_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    event_type TEXT NOT NULL,
    ip_hint TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT auth_events_type_chk CHECK (
        event_type IN (
            'login',
            'register',
            'logout',
            'session_revoke',
            'session_revoke_others',
            'profile_update',
            'data_request'
        )
    )
);

CREATE INDEX IF NOT EXISTS auth_events_user_created_idx
    ON auth_events (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS profile_data_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    request_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT profile_data_req_type_chk CHECK (
        request_type IN ('personal_data', 'address', 'contacts', 'other')
    ),
    CONSTRAINT profile_data_req_status_chk CHECK (
        status IN ('open', 'in_review', 'done', 'rejected')
    )
);

CREATE INDEX IF NOT EXISTS profile_data_requests_user_idx
    ON profile_data_requests (user_id, created_at DESC);
