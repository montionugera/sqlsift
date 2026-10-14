//! E0035 duplicate-insert-value: Duplicate primary key or unique constraint
//! values within one INSERT ... VALUES list.
//!
//! Two rows of the same statement colliding on a primary key or unique constraint
//! always fail at runtime (unless conflict handling or replacement is specified).

use sqlsift_core::dialect::SqlDialect;
use sqlsift_core::schema::SchemaBuilder;
use sqlsift_core::Analyzer;

const SCHEMA: &str = "
CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);
CREATE TABLE memberships (org_id INT, user_id INT, role TEXT, PRIMARY KEY (org_id, user_id));
CREATE TABLE named_keys (id INT, code TEXT, CONSTRAINT uq_code UNIQUE (code));
CREATE TABLE pk_and_unique (id INT PRIMARY KEY UNIQUE, val TEXT);
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
        found.contains(&"E0035"),
        "expected E0035 for {sql}, got {found:?}"
    );
}

#[track_caller]
fn assert_clean(sql: &str) {
    assert_eq!(pg_codes(sql), Vec::<&str>::new(), "{sql}");
}

#[test]
fn e0035_duplicate_primary_key() {
    assert_reports("INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')");
    assert_reports("INSERT INTO users (id) VALUES (42), (42)");
}

#[test]
fn e0035_duplicate_unique_constraint() {
    assert_reports("INSERT INTO users (id, email) VALUES (2, 'a@x'), (3, 'a@x')");
    assert_reports("INSERT INTO named_keys (id, code) VALUES (1, 'ALPHA'), (2, 'ALPHA')");
}

#[test]
fn e0035_composite_primary_key() {
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
fn e0035_positional_insert() {
    assert_reports("INSERT INTO users VALUES (1, 'a@x'), (1, 'b@x')");
    assert_clean("INSERT INTO users VALUES (1, 'a@x'), (2, 'b@x')");
}

#[test]
fn e0035_on_conflict_do_nothing() {
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

    // ON CONFLICT DO UPDATE in PostgreSQL still fails at runtime (cannot affect row a second time)
    assert_reports(
        "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x') ON CONFLICT (id) DO UPDATE SET email = EXCLUDED.email"
    );
}

#[test]
fn e0035_insert_ignore_dialects() {
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
fn e0035_mysql_accepted_forms() {
    // MySQL ON DUPLICATE KEY UPDATE: later row wins
    assert_eq!(
        codes_in(
            SqlDialect::MySQL,
            "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x') ON DUPLICATE KEY UPDATE email = 'updated'"
        ),
        Vec::<&str>::new()
    );

    // MySQL REPLACE INTO: later row replaces
    assert_eq!(
        codes_in(
            SqlDialect::MySQL,
            "REPLACE INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );
}

#[test]
fn e0035_sqlite_accepted_forms() {
    // SQLite INSERT OR REPLACE: later row replaces
    assert_eq!(
        codes_in(
            SqlDialect::SQLite,
            "INSERT OR REPLACE INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );

    // SQLite ON CONFLICT (id) DO UPDATE stays quiet (SQLite allows later row to update)
    assert_eq!(
        codes_in(
            SqlDialect::SQLite,
            "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x') ON CONFLICT (id) DO UPDATE SET email = excluded.email"
        ),
        Vec::<&str>::new()
    );
}

#[test]
fn e0035_postgres_on_conflict_on_constraint_unnamed() {
    // ON CONFLICT ON CONSTRAINT users_pkey DO NOTHING stays quiet even if the schema
    // didn't explicitly name the constraint
    assert_clean(
        "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x') ON CONFLICT ON CONSTRAINT users_pkey DO NOTHING"
    );
}

#[test]
fn e0035_hex_literals() {
    // MySQL: X'41' decodes to 'A', not '41'
    // X'41' vs '41' does not collide: clean
    assert_eq!(
        codes_in(
            SqlDialect::MySQL,
            "INSERT INTO users (id, email) VALUES (1, X'41'), (2, '41')"
        ),
        Vec::<&str>::new()
    );

    // X'41' vs 'A' is duplicate: reports E0035
    let diags = codes_in(
        SqlDialect::MySQL,
        "INSERT INTO users (id, email) VALUES (1, X'41'), (2, 'A')",
    );
    assert_eq!(diags, vec!["E0035"]);

    // X'41' vs X'41' is duplicate: reports E0035
    let diags = codes_in(
        SqlDialect::MySQL,
        "INSERT INTO users (id, email) VALUES (1, X'41'), (2, X'41')",
    );
    assert_eq!(diags, vec!["E0035"]);
}

#[test]
fn e0035_column_both_pk_and_unique_reports_once() {
    let mut builder = SchemaBuilder::with_dialect(SqlDialect::PostgreSQL);
    builder.parse(SCHEMA).expect("schema parses");
    let catalog = builder.build().0;
    let mut analyzer = Analyzer::with_dialect(&catalog, SqlDialect::PostgreSQL);

    let diags = analyzer.analyze("INSERT INTO pk_and_unique (id, val) VALUES (1, 'a'), (1, 'b')");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0035");
    assert!(diags[0].message.contains("Duplicate primary key value 1"));
}

#[test]
fn e0035_null_in_unique_column_is_clean() {
    // In SQL standard & Postgres, NULL != NULL for unique constraints
    assert_clean("INSERT INTO users (id, email) VALUES (1, NULL), (2, NULL)");
}

#[test]
fn e0035_diagnostics_message_content() {
    let mut builder = SchemaBuilder::with_dialect(SqlDialect::PostgreSQL);
    builder.parse(SCHEMA).expect("schema parses");
    let catalog = builder.build().0;
    let mut analyzer = Analyzer::with_dialect(&catalog, SqlDialect::PostgreSQL);

    let diags = analyzer.analyze("INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0035");
    assert!(diags[0].message.contains("Duplicate primary key value 1"));
    assert!(diags[0].message.contains("table 'users'"));
}

#[test]
fn e0035_dialect_gating() {
    // Redshift, Snowflake, BigQuery, Databricks don't enforce primary or unique keys: stay quiet
    assert_eq!(
        codes_in(
            SqlDialect::Redshift,
            "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );
    assert_eq!(
        codes_in(
            SqlDialect::Snowflake,
            "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );
    assert_eq!(
        codes_in(
            SqlDialect::BigQuery,
            "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );
    assert_eq!(
        codes_in(
            SqlDialect::Databricks,
            "INSERT INTO users (id, email) VALUES (1, 'a@x'), (1, 'b@x')"
        ),
        Vec::<&str>::new()
    );
}

#[test]
fn e0035_hex_literals_non_mysql() {
    // In PostgreSQL and SQLite, X'41' is a bit string / blob and not text 'A'.
    // They do not collide: clean
    assert_clean("INSERT INTO users (id, email) VALUES (1, X'41'), (2, 'A')");
    assert_eq!(
        codes_in(
            SqlDialect::SQLite,
            "INSERT INTO users (id, email) VALUES (1, X'41'), (2, 'A')"
        ),
        Vec::<&str>::new()
    );
}

#[test]
fn e0035_mysql_auto_increment_zero_is_clean() {
    let schema = "CREATE TABLE items (id INT AUTO_INCREMENT PRIMARY KEY, name TEXT);";
    let mut builder = SchemaBuilder::with_dialect(SqlDialect::MySQL);
    builder.parse(schema).expect("schema parses");
    let catalog = builder.build().0;
    let mut analyzer = Analyzer::with_dialect(&catalog, SqlDialect::MySQL);

    // In MySQL, inserting 0 into an AUTO_INCREMENT column generates new ids (acts like NULL)
    let diags = analyzer.analyze("INSERT INTO items (id, name) VALUES (0, 'x'), (0, 'y')");
    assert_eq!(diags.len(), 0);

    // Inserting identical non-zero values still reports duplicate
    let diags = analyzer.analyze("INSERT INTO items (id, name) VALUES (1, 'x'), (1, 'y')");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0035");
}

#[test]
fn e0035_deferrable_initially_deferred_keys_skipped() {
    let schema = "
CREATE TABLE def_pk (id INT CONSTRAINT pk_def PRIMARY KEY DEFERRABLE INITIALLY DEFERRED, val TEXT);
CREATE TABLE def_uq (id INT, val TEXT, CONSTRAINT uq_def UNIQUE (val) DEFERRABLE INITIALLY DEFERRED);
";
    let mut builder = SchemaBuilder::with_dialect(SqlDialect::PostgreSQL);
    builder.parse(schema).expect("schema parses");
    let catalog = builder.build().0;
    let mut analyzer = Analyzer::with_dialect(&catalog, SqlDialect::PostgreSQL);

    // DEFERRABLE INITIALLY DEFERRED primary key: skipped
    let diags = analyzer.analyze("INSERT INTO def_pk (id, val) VALUES (1, 'a'), (1, 'b')");
    assert_eq!(diags.len(), 0);

    // DEFERRABLE INITIALLY DEFERRED unique constraint: skipped
    let diags = analyzer.analyze("INSERT INTO def_uq (id, val) VALUES (1, 'x'), (2, 'x')");
    assert_eq!(diags.len(), 0);
}
