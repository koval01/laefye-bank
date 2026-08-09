-- Document Security Module (DSM) — TSE-inspired sealed bank documents.
-- Monotonic signature counter + hash chain; not a certified BSI hardware TSE.

CREATE TABLE IF NOT EXISTS document_dsm (
    id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    signature_counter BIGINT NOT NULL DEFAULT 0,
    last_signature_b64 TEXT NOT NULL DEFAULT '',
    serial TEXT NOT NULL,
    public_key_b64 TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS document_seals (
    id BIGSERIAL PRIMARY KEY,
    doc_type TEXT NOT NULL,
    doc_ref TEXT NOT NULL,
    transaction_number BIGINT NOT NULL,
    signature_counter BIGINT NOT NULL,
    process_type TEXT NOT NULL,
    process_data TEXT NOT NULL,
    signed_at TIMESTAMPTZ NOT NULL,
    signature_b64 TEXT NOT NULL,
    previous_signature_b64 TEXT NOT NULL DEFAULT '',
    qr_payload TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (doc_type, doc_ref)
);

CREATE INDEX IF NOT EXISTS document_seals_sig_counter_idx
    ON document_seals (signature_counter DESC);
