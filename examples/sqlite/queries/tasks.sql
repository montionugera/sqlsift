-- Fetch active tasks with priority and due date for a specific user
SELECT t.id, t.title, t.priority, t.due_date
FROM tasks t
JOIN users u ON u.id = t.user_id
WHERE u.id = ? AND u.active = 1 AND t.completed = 0
ORDER BY t.priority DESC, t.due_date ASC;

-- Insert or replace a task tag (SQLite-specific syntax)
INSERT OR REPLACE INTO task_tags (task_id, tag)
VALUES (?, ?);

-- Upsert a task using ON CONFLICT DO UPDATE (SQLite 3.24+)
INSERT INTO tasks (id, user_id, title, priority, due_date)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT (id) DO UPDATE SET
    title = excluded.title,
    priority = excluded.priority,
    due_date = excluded.due_date;
