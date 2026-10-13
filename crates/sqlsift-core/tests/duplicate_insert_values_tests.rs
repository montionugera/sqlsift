//! E0032 duplicate-insert-value: Duplicate primary key or unique constraint
//! values within one INSERT ... VALUES list.
//!
//! Two rows of the same statement colliding on a primary key or unique constraint
//! always fail at runtime (unless ON CONFLICT DO NOTHING / INSERT IGNORE).

use sqlsift_core::dialect::SqlDialect;
use sqlsift_core::schema::SchemaBuilder;
use sqlsift_core::Analyzer;

const SCHEMA: &str = "
CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);
CREATE TABLE memberships (org_id INT, user_id INT, role TEXT, PRIMARY KEY (org_id, user_id));
CREATE TABLE named_keys (id INT, code TEXT, CONSTRAINT uq_code UNIQUE (code));
";

fn codes_in(dialect: SqlDialect, sql: &str) -> Vec<&'static str> {
    let mut builder = SchemaBuilder::with_dialect(dialect);
    builder.parse(SCHEMA).expect("schema parses");
    let catalog = builder.build().0;
    Analyzer::with_dialect(&catalog, dialect)
        .analyze(sql)
        .iter()
        .map(sqlsift_core::Diagnostic::code)
        .collect()
}

fn pg_codes(sql: &str) -> Vec<&'static str> {
    codes_in(SqlDialect::PostgreSQL, sql)
}

#[track_caller]
fn assert_reports(sql: &str) {
    let found = pg_codes(sql);
    assert!(
        found.contains(&"E0032"),
        "expected E0032 for {sql}, got {found:?}"
    );
}

#[track_caller]
fn assert_clean(sql: &str) {
    assert_eq!(pg_codes(sql), Vec::<&str>::new(), "{sql}");
}

#[test]
fn e0032_duplicate_primary_key() {
    assert_reports("INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')");
    assert_reports("INSERT INTO users (id) VALUES (42), (42)");
}

#[test]
fn e0032_duplicate_unique_constraint() {
    assert_reports("INSERT INTO users (id, email) VALUES (2, 'a@x'), (3, 'a@x')");
    assert_reports("INSERT INTO named_keys (id, code) VALUES (1, 'ALPHA'), (2, 'ALPHA')");
}

#[test]
fn e0032_composite_primary_key() {
    // Different composite keys: clean
    assert_clean(
        "INSERT INTO memberships (org_id, user_id, role) VALUES (1, 10, 'admin'), (1, 20, 'member')"
    );
    // Identical composite keys: reported
    assert_reports(
        "INSERT INTO memberships (org_id, user_id, role) VALUES (1, 10, 'admin'), (1, 10, 'member')"
    );
}

#[test]
fn e0032_positional_insert() {
    assert_reports("INSERT INTO users VALUES (1, 'a@x'), (1, 'b@x')");
    assert_clean("INSERT INTO users VALUES (1, 'a@x'), (2, 'b@x')");
}

#[test]
fn e0032_on_conflict_do_nothing() {
    // ON CONFLICT (id) DO NOTHING suppresses duplicate on id
    assert_clean(
        "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x') ON CONFLICT (id) DO NOTHING",
    );

    // Bare ON CONFLICT DO NOTHING suppresses all unique conflicts
    assert_clean(
        "INSERT INTO users (id, email) VALUES (1, 'a@x'), (2, 'a@x') ON CONFLICT DO NOTHING",
    );

    // ON CONFLICT on constraint name suppresses conflict
    assert_clean(
        "INSERT INTO named_keys (id, code) VALUES (1, 'ALPHA'), (2, 'ALPHA') ON CONFLICT ON CONSTRAINT uq_code DO NOTHING"
    );

    // ON CONFLICT (id) DO NOTHING does NOT suppress conflict on email
    assert_reports(
        "INSERT INTO users (id, email) VALUES (1, 'a@x'), (2, 'a@x') ON CONFLICT (id) DO NOTHING",
    );

    // ON CONFLICT DO UPDATE still fails at runtime (cannot affect row a second time)
    assert_reports(
        "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x') ON CONFLICT (id) DO UPDATE SET email = EXCLUDED.email"
    );
}

#[test]
fn e0032_insert_ignore_dialects() {
    // MySQL INSERT IGNORE
    assert_eq!(
        codes_in(
            SqlDialect::MySQL,
            "INSERT IGNORE INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );

    // SQLite INSERT OR IGNORE
    assert_eq!(
        codes_in(
            SqlDialect::SQLite,
            "INSERT OR IGNORE INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );
}

#[test]
fn e0032_null_in_unique_column_is_clean() {
    // In SQL standard & Postgres, NULL != NULL for unique constraints
    assert_clean("INSERT INTO users (id, email) VALUES (1, NULL), (2, NULL)");
}

#[test]
fn e0032_diagnostics_message_content() {
    let mut builder = SchemaBuilder::with_dialect(SqlDialect::PostgreSQL);
    builder.parse(SCHEMA).expect("schema parses");
    let catalog = builder.build().0;
    let mut analyzer = Analyzer::with_dialect(&catalog, SqlDialect::PostgreSQL);

    let diags = analyzer.analyze("INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0032");
    assert!(diags[0].message.contains("Duplicate primary key value 1"));
    assert!(diags[0].message.contains("table 'users'"));
}
