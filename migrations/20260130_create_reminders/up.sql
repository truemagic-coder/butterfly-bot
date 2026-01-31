CREATE TABLE IF NOT EXISTS reminders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    title TEXT NOT NULL,
    due_at BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    completed_at BIGINT,
    fired_at BIGINT
);

CREATE INDEX IF NOT EXISTS idx_reminders_user_due
    ON reminders(user_id, due_at);
