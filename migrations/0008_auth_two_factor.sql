CREATE TABLE IF NOT EXISTS auth_two_factor_challenges (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    resend_count INTEGER NOT NULL DEFAULT 0,
    last_resend_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_two_factor_challenges_user_id ON auth_two_factor_challenges(user_id);
CREATE INDEX IF NOT EXISTS idx_two_factor_challenges_expires_at ON auth_two_factor_challenges(expires_at);
