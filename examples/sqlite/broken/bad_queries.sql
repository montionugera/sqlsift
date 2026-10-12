-- Table 'reminders' does not exist in schema.sql.
SELECT id, title
FROM reminders
WHERE user_id = ?;

-- Column 'user_name' does not exist in tasks (author is in users via user_id).
SELECT t.id, t.user_name
FROM tasks t
WHERE t.id = ?;

-- Column 'user_id' does not exist in table 'users': the primary key is 'id'.
SELECT t.id, u.username
FROM tasks t
JOIN users u ON u.user_id = t.user_id;

-- Column 'title' has no unique constraint or primary key to match the ON CONFLICT target.
INSERT INTO tasks (id, user_id, title)
VALUES (?, ?, ?)
ON CONFLICT (title) DO UPDATE SET title = excluded.title;
