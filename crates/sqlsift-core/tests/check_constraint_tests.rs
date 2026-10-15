//! E0037 check-constraint-violation: literals that violate a CHECK constraint in
//! INSERT VALUES or UPDATE SET.

use sqlsift_core::dialect::SqlDialect;
use sqlsift_core::schema::SchemaBuilder;
use sqlsift_core::Analyzer;

const SCHEMA: &str = "
CREATE TABLE users (
    id INT PRIMARY KEY,
    age INT CONSTRAINT users_age_check CHECK (age >= 0),
    status TEXT CONSTRAINT users_status_check CHECK (status IN ('active', 'banned')),
    bio TEXT CHECK (length(bio) <= 20)
);

CREATE TABLE ranges (
    id INT PRIMARY KEY,
    lo INT,
    hi INT,
    CONSTRAINT lo_le_hi CHECK (lo <= hi)
);

CREATE TABLE orders (
    id INT PRIMARY KEY,
    qty INT CHECK (qty BETWEEN 1 AND 100),
    discount INT CHECK (discount >= 0 AND discount <= 50),
    category TEXT CHECK (category NOT IN ('archived', 'deleted'))
);
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

#[track_caller]
fn assert_reports(sql: &str) {
    assert_eq!(codes_in(SqlDialect::PostgreSQL, sql), ["E0037"], "{sql}");
}

#[track_caller]
fn assert_clean(sql: &str) {
    assert_eq!(
        codes_in(SqlDialect::PostgreSQL, sql),
        Vec::<&str>::new(),
        "{sql}"
    );
}

#[test]
fn single_column_comparison_check() {
    assert_reports("INSERT INTO users (id, age) VALUES (1, -1)");
    assert_clean("INSERT INTO users (id, age) VALUES (1, 0)");
    assert_clean("INSERT INTO users (id, age) VALUES (1, 25)");
    // NULL satisfies a CHECK constraint in SQL
    assert_clean("INSERT INTO users (id, age) VALUES (1, NULL)");
}

#[test]
fn update_set_violates_check() {
    assert_reports("UPDATE users SET status = 'actve' WHERE id = 1");
    assert_clean("UPDATE users SET status = 'active' WHERE id = 1");
    assert_clean("UPDATE users SET status = 'banned' WHERE id = 1");
    assert_clean("UPDATE users SET status = NULL WHERE id = 1");
}

#[test]
fn positional_insert_without_columns() {
    assert_reports("INSERT INTO users VALUES (1, -5, 'active', 'hello')");
    assert_clean("INSERT INTO users VALUES (1, 20, 'active', 'hello')");
}

#[test]
fn multi_row_insert_reports_violating_row() {
    let diags = codes_in(
        SqlDialect::PostgreSQL,
        "INSERT INTO users (id, age) VALUES (1, 20), (2, -1), (3, 30)",
    );
    assert_eq!(diags, vec!["E0037"]);
}

#[test]
fn multi_column_table_constraint() {
    // Both columns assigned and lo > hi -> violates lo <= hi
    assert_reports("INSERT INTO ranges (id, lo, hi) VALUES (1, 10, 5)");
    assert_clean("INSERT INTO ranges (id, lo, hi) VALUES (1, 5, 10)");
    assert_clean("INSERT INTO ranges (id, lo, hi) VALUES (1, 5, 5)");

    // Only one column assigned: check is unknown, not reported
    assert_clean("INSERT INTO ranges (id, lo) VALUES (1, 10)");
    assert_clean("UPDATE ranges SET lo = 20 WHERE id = 1");

    // Both updated: lo > hi
    assert_reports("UPDATE ranges SET lo = 20, hi = 10 WHERE id = 1");
    assert_clean("UPDATE ranges SET lo = 5, hi = 10 WHERE id = 1");
}

#[test]
fn between_and_in_list_checks() {
    assert_reports("INSERT INTO orders (id, qty) VALUES (1, 0)");
    assert_reports("INSERT INTO orders (id, qty) VALUES (1, 101)");
    assert_clean("INSERT INTO orders (id, qty) VALUES (1, 1)");
    assert_clean("INSERT INTO orders (id, qty) VALUES (1, 50)");
    assert_clean("INSERT INTO orders (id, qty) VALUES (1, 100)");

    assert_reports("INSERT INTO orders (id, discount) VALUES (1, 60)");
    assert_clean("INSERT INTO orders (id, discount) VALUES (1, 25)");

    assert_reports("INSERT INTO orders (id, category) VALUES (1, 'archived')");
    assert_reports("INSERT INTO orders (id, category) VALUES (1, 'deleted')");
    assert_clean("INSERT INTO orders (id, category) VALUES (1, 'electronics')");
}

#[test]
fn string_length_check() {
    assert_reports(
        "INSERT INTO users (id, bio) VALUES (1, 'This bio is way too long for twenty chars!')",
    );
    assert_clean("INSERT INTO users (id, bio) VALUES (1, 'Short bio')");
}

#[test]
fn dialect_enforcement() {
    let sql = "INSERT INTO users (id, age) VALUES (1, -1)";

    // PostgreSQL, MySQL, and SQLite enforce CHECK constraints
    assert_eq!(codes_in(SqlDialect::PostgreSQL, sql), vec!["E0037"]);
    assert_eq!(codes_in(SqlDialect::MySQL, sql), vec!["E0037"]);
    assert_eq!(codes_in(SqlDialect::SQLite, sql), vec!["E0037"]);

    // Warehouses treat CHECK constraints as informational / unenforced
    assert_eq!(codes_in(SqlDialect::Snowflake, sql), Vec::<&str>::new());
    assert_eq!(codes_in(SqlDialect::Redshift, sql), Vec::<&str>::new());
}

#[test]
fn diagnostic_message_and_help() {
    let mut builder = SchemaBuilder::with_dialect(SqlDialect::PostgreSQL);
    builder.parse(SCHEMA).expect("schema parses");
    let catalog = builder.build().0;
    let diags = Analyzer::with_dialect(&catalog, SqlDialect::PostgreSQL)
        .analyze("INSERT INTO users (id, age) VALUES (1, -1)");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0037");
    assert!(
        diags[0].message.contains("users_age_check"),
        "expected constraint name in message: {}",
        diags[0].message
    );
    assert!(
        diags[0].help.as_ref().unwrap().contains("age >= 0"),
        "expected expression in help: {:?}",
        diags[0].help
    );
}
