-- KYC metadata only — ciphertext lives in object storage.

CREATE TABLE IF NOT EXISTS kyc_cases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    reviewer_note TEXT NOT NULL DEFAULT '',
    reviewed_by UUID,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT kyc_status_chk CHECK (
        status IN ('draft', 'pending', 'approved', 'rejected', 'resubmit')
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS kyc_one_open_case
    ON kyc_cases (user_id)
    WHERE status IN ('draft', 'pending', 'resubmit');

CREATE INDEX IF NOT EXISTS kyc_cases_user_idx ON kyc_cases (user_id);
CREATE INDEX IF NOT EXISTS kyc_cases_status_idx ON kyc_cases (status);

CREATE TABLE IF NOT EXISTS kyc_documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id UUID NOT NULL REFERENCES kyc_cases (id) ON DELETE CASCADE,
    user_id UUID NOT NULL,
    doc_type TEXT NOT NULL,
    -- Random object key; ciphertext only in MinIO. Never log this with user PII together in cleartext paths.
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    byte_size INT NOT NULL CHECK (byte_size > 0 AND byte_size <= 10485760),
    -- SHA-256 of plaintext for integrity (not reversible).
    plaintext_sha256 BYTEA NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT kyc_doc_type_chk CHECK (
        doc_type IN ('passport', 'selfie', 'address_proof', 'other')
    )
);

CREATE TABLE IF NOT EXISTS kyc_audit (
    id BIGSERIAL PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    actor_user_id UUID,
    action TEXT NOT NULL,
    case_id UUID,
    document_id UUID,
    detail TEXT NOT NULL DEFAULT ''
);
