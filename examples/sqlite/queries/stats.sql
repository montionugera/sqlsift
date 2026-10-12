-- Monthly completed task count using SQLite strftime()
SELECT
    u.username,
    strftime('%Y-%m', t.created_at) AS month,
    count(*) AS completed_count
FROM tasks t
JOIN users u ON u.id = t.user_id
WHERE t.completed = 1
GROUP BY u.username, month;
