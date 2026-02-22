CREATE TABLE IF NOT EXISTS plan_step_dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id INTEGER NOT NULL,
    user_id TEXT NOT NULL,
    step_ref TEXT NOT NULL,
    depends_on_ref TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(plan_id, step_ref, depends_on_ref)
);

CREATE INDEX IF NOT EXISTS plan_step_dependencies_user_idx
ON plan_step_dependencies (user_id, plan_id, step_ref, updated_at);
