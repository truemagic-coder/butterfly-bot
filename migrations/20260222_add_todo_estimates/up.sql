CREATE TABLE IF NOT EXISTS todo_items (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	user_id TEXT NOT NULL,
	title TEXT NOT NULL,
	notes TEXT,
	position INTEGER NOT NULL,
	created_at INTEGER NOT NULL,
	updated_at INTEGER NOT NULL,
	completed_at INTEGER
);

ALTER TABLE todo_items ADD COLUMN t_shirt_size TEXT;
ALTER TABLE todo_items ADD COLUMN story_points INTEGER;
ALTER TABLE todo_items ADD COLUMN estimate_optimistic_minutes INTEGER;
ALTER TABLE todo_items ADD COLUMN estimate_likely_minutes INTEGER;
ALTER TABLE todo_items ADD COLUMN estimate_pessimistic_minutes INTEGER;
