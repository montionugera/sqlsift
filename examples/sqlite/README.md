# SQLite

A SQLite schema in `schema.sql`, with `INTEGER PRIMARY KEY` (rowid aliases), booleans stored as integers (`WHERE active = 1`), `datetime('now')` defaults, `strftime()` functions, and SQLite-specific syntax like `INSERT OR REPLACE` and `INSERT ... ON CONFLICT DO UPDATE`.

```text
sqlsift.toml
schema.sql
queries/tasks.sql
queries/stats.sql
broken/bad_queries.sql     intentional mistakes
```

`sqlsift.toml`:

```toml
schema = ["schema.sql"]
files = ["queries/**/*.sql"]
dialect = "sqlite"
```

## Run it

```console
$ sqlsift check
All 2 file(s) passed validation
```

## What it catches

```console
$ sqlsift check broken/bad_queries.sql
error[E0001]: Table 'reminders' not found
  --> broken/bad_queries.sql:3:6
    |
  3 | FROM reminders
    |      ^^^^^^^^^
    = help: Check that the table exists in your schema definition; run `sqlsift schema <schema files>` to list the tables that were loaded

error[E0002]: Column 'user_name' not found in table 'tasks'
  --> broken/bad_queries.sql:7:16
    |
  7 | SELECT t.id, t.user_name
    |                ^^^^^^^^^

error[E0002]: Column 'user_id' not found in table 'users'
  --> broken/bad_queries.sql:14:19
    |
 14 | JOIN users u ON u.user_id = t.user_id;
    |                   ^^^^^^^

error[E0014]: No unique constraint or primary key of 'tasks' matches ON CONFLICT (title)
  --> broken/bad_queries.sql:19:14
    |
 19 | ON CONFLICT (title) DO UPDATE SET title = excluded.title;
    |              ^^^^^
    = help: The unique keys of 'tasks' are (id); add a unique constraint or index on (title)

Found 4 error(s), 0 warning(s) in 1 file(s)
```

SQLite allows flexible column typing and bare columns in queries, but missing tables, invalid column names in joins and SELECTs, and mismatched conflict targets still cause runtime errors. `sqlsift` catches these ahead of time without needing an active database connection.

CI checks both commands on every change to sqlsift (`scripts/check-examples.sh`, against `expected-diagnostics.txt`).
