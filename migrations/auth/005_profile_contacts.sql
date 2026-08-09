-- Contact / identity fields for basic account use (email, phone, FIO, DOB).

ALTER TABLE bank_profiles
    ADD COLUMN IF NOT EXISTS email TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS phone TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS full_name TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS birth_date DATE;

COMMENT ON COLUMN bank_profiles.email IS 'Contact email (optional until filled)';
COMMENT ON COLUMN bank_profiles.phone IS 'E.164-ish phone string';
COMMENT ON COLUMN bank_profiles.full_name IS 'Legal full name (FIO)';
COMMENT ON COLUMN bank_profiles.birth_date IS 'Date of birth';
