CREATE TABLE IF NOT EXISTS inbox_item_states (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    origin_ref TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(user_id, origin_ref)
);

CREATE INDEX IF NOT EXISTS inbox_item_states_user_status_idx
ON inbox_item_states (user_id, status, updated_at);
