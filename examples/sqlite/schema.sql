-- SQLite database schema for a task and note tracking system.

CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE tasks (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    title TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 1 CHECK (priority BETWEEN 1 AND 5),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    due_date TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE task_tags (
    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);
