//! Pedantic rules (off by default): stricter checks that may have false positives
//!
//! - E0032 integer-division: division of integer columns truncates toward zero
//! - E0034 case-enum-missing-labels: `CASE` over an enum column that misses some
//!   labels and has no `ELSE` clause

use sqlsift_core::dialect::SqlDialect;
use sqlsift_core::error::Severity;
use sqlsift_core::rules::{RuleConfig, RuleLevel};
use sqlsift_core::schema::SchemaBuilder;
use sqlsift_core::Analyzer;

// ---------------------------------------------------------------------------
// E0032: Integer division test fixtures and helpers
// ---------------------------------------------------------------------------

const DIVISION_SCHEMA: &str = "
CREATE TABLE orders (id INT PRIMARY KEY, qty INT, items INT, price NUMERIC);
";

fn diagnostics_in(
    dialect: SqlDialect,
    rules: RuleConfig,
    sql: &str,
) -> Vec<sqlsift_core::Diagnostic> {
    let mut builder = SchemaBuilder::with_dialect(dialect);
    builder.parse(DIVISION_SCHEMA).expect("schema parses");
    let catalog = builder.build().0;
    Analyzer::with_dialect(&catalog, dialect)
        .with_rules(rules)
        .analyze(sql)
}

fn codes_in(dialect: SqlDialect, rules: RuleConfig, sql: &str) -> Vec<&'static str> {
    diagnostics_in(dialect, rules, sql)
        .iter()
        .map(sqlsift_core::Diagnostic::code)
        .collect()
}

fn division_codes(sql: &str) -> Vec<&'static str> {
    codes_in(SqlDialect::PostgreSQL, pedantic_rules(), sql)
}

// ---------------------------------------------------------------------------
// E0034: CASE enum missing labels test fixtures and helpers
// ---------------------------------------------------------------------------

const PG_SCHEMA: &str = "
CREATE TYPE order_status AS ENUM ('pending', 'paid', 'shipped', 'cancelled');
CREATE TABLE orders (id INT PRIMARY KEY, status order_status NOT NULL);
";

const MYSQL_SCHEMA: &str = "
CREATE TABLE orders (id INT PRIMARY KEY, status ENUM('pending', 'paid', 'shipped', 'cancelled') NOT NULL);
";

fn diagnostics_with(
    dialect: SqlDialect,
    schema: &str,
    rules: RuleConfig,
    sql: &str,
) -> Vec<sqlsift_core::Diagnostic> {
    let mut builder = SchemaBuilder::with_dialect(dialect);
    builder.parse(schema).expect("schema parses");
    let catalog = builder.build().0;
    Analyzer::with_dialect(&catalog, dialect)
        .with_rules(rules)
        .analyze(sql)
}

fn pedantic_rules() -> RuleConfig {
    let mut rules = RuleConfig::default();
    rules.configure("pedantic", RuleLevel::Warn).unwrap();
    rules
}

fn codes(sql: &str) -> Vec<&'static str> {
    diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql)
        .iter()
        .map(sqlsift_core::Diagnostic::code)
        .collect()
}

// ===========================================================================
// E0032 Tests
// ===========================================================================

#[test]
fn e0032_off_by_default() {
    let default_rules = RuleConfig::default();
    assert!(codes_in(
        SqlDialect::PostgreSQL,
        default_rules.clone(),
        "SELECT qty / items FROM orders"
    )
    .is_empty());
    assert!(codes_in(
        SqlDialect::PostgreSQL,
        default_rules.clone(),
        "SELECT count(*) / count(DISTINCT id) FROM orders"
    )
    .is_empty());
    assert!(codes_in(
        SqlDialect::PostgreSQL,
        default_rules,
        "SELECT qty / 3 FROM orders"
    )
    .is_empty());
}

#[test]
fn e0032_column_by_column() {
    let diags = diagnostics_in(
        SqlDialect::PostgreSQL,
        pedantic_rules(),
        "SELECT qty / items AS ratio FROM orders",
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0032");
    assert_eq!(
        diags[0].message,
        "Division of integer columns truncates toward zero: integer / integer produces an integer"
    );
    assert_eq!(
        diags[0].help.as_deref(),
        Some(
            "Cast an operand to NUMERIC or FLOAT if a ratio or average is meant: CAST(x AS NUMERIC) / y"
        )
    );
}

#[test]
fn e0032_column_and_literal() {
    assert_eq!(division_codes("SELECT qty / 3 FROM orders"), ["E0032"]);
    assert_eq!(division_codes("SELECT 10 / items FROM orders"), ["E0032"]);
}

#[test]
fn e0032_two_literals_are_clean() {
    assert!(division_codes("SELECT 10 / 3").is_empty());
    assert!(division_codes("SELECT 10 / 3 FROM orders").is_empty());
    assert!(division_codes("SELECT -10 / 3").is_empty());
    assert!(division_codes("SELECT (10 + 2) / 3").is_empty());
}

#[test]
fn e0032_aggregates() {
    let diags = diagnostics_in(
        SqlDialect::PostgreSQL,
        pedantic_rules(),
        "SELECT count(*) / count(DISTINCT id) FROM orders",
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0032");
    assert_eq!(
        diags[0].message,
        "Division of integer columns truncates toward zero: bigint / bigint produces an integer"
    );

    assert_eq!(
        division_codes("SELECT count(*) / 10 FROM orders"),
        ["E0032"]
    );
    assert_eq!(
        division_codes("SELECT 100 / count(*) FROM orders"),
        ["E0032"]
    );
}

#[test]
fn e0032_numeric_or_cast_is_clean() {
    assert!(division_codes("SELECT qty::numeric / items FROM orders").is_empty());
    assert!(division_codes("SELECT CAST(qty AS NUMERIC) / items FROM orders").is_empty());
    assert!(division_codes("SELECT qty / CAST(items AS FLOAT) FROM orders").is_empty());
    assert!(division_codes("SELECT price / items FROM orders").is_empty());
    assert!(division_codes("SELECT (1.0 * qty) / items FROM orders").is_empty());
}

#[test]
fn e0032_inner_division_in_cast_is_reported() {
    // Truncates before the cast to numeric
    assert_eq!(
        division_codes("SELECT (qty / items)::numeric FROM orders"),
        ["E0032"]
    );
}

#[test]
fn e0032_configured_by_name_or_code() {
    let mut by_name = RuleConfig::default();
    by_name
        .configure("integer-division", RuleLevel::Warn)
        .unwrap();
    assert_eq!(
        codes_in(
            SqlDialect::PostgreSQL,
            by_name,
            "SELECT qty / items FROM orders"
        ),
        ["E0032"]
    );

    let mut by_code = RuleConfig::default();
    by_code.configure("E0032", RuleLevel::Error).unwrap();
    let diags = diagnostics_in(
        SqlDialect::PostgreSQL,
        by_code,
        "SELECT qty / items FROM orders",
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0032");
    assert_eq!(diags[0].severity, sqlsift_core::error::Severity::Error);
}

#[test]
fn e0032_mysql_does_not_truncate_division() {
    // In MySQL, / returns decimal, so it does not truncate integer division
    assert!(codes_in(
        SqlDialect::MySQL,
        pedantic_rules(),
        "SELECT qty / items FROM orders"
    )
    .is_empty());
}

#[test]
fn e0032_sqlite_reports() {
    // In SQLite, / between integers truncates
    assert_eq!(
        codes_in(
            SqlDialect::SQLite,
            pedantic_rules(),
            "SELECT qty / items FROM orders"
        ),
        ["E0032"]
    );
}

// ===========================================================================
// E0034 Tests
// ===========================================================================

#[test]
fn e0034_off_by_default() {
    let sql = "SELECT CASE status WHEN 'pending' THEN 'Waiting' END FROM orders";
    let default_diags = diagnostics_with(
        SqlDialect::PostgreSQL,
        PG_SCHEMA,
        RuleConfig::default(),
        sql,
    );
    assert!(
        default_diags.is_empty(),
        "Pedantic rules should be off by default"
    );
}

#[test]
fn e0034_simple_case_missing_labels() {
    let sql = "SELECT CASE status WHEN 'pending' THEN 'Waiting' WHEN 'paid' THEN 'Paid' WHEN 'shipped' THEN 'On its way' END FROM orders";
    let diags = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0034");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(
        diags[0].message,
        "CASE over enum column 'orders.status' does not handle: 'cancelled'"
    );
    assert_eq!(
        diags[0].help.as_deref(),
        Some("Add WHEN branches for the missing labels or an ELSE clause to handle them.")
    );

    // Multiple missing labels
    let sql2 = "SELECT CASE status WHEN 'pending' THEN 'Waiting' END FROM orders";
    let diags2 = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql2);
    assert_eq!(diags2.len(), 1, "{diags2:?}");
    assert_eq!(
        diags2[0].message,
        "CASE over enum column 'orders.status' does not handle: 'paid', 'shipped', 'cancelled'"
    );
}

#[test]
fn e0034_searched_case_missing_labels() {
    let sql = "SELECT CASE WHEN status = 'pending' THEN 'Waiting' WHEN status = 'paid' THEN 'Paid' WHEN status = 'shipped' THEN 'On its way' END FROM orders";
    let diags = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0034");
    assert_eq!(
        diags[0].message,
        "CASE over enum column 'orders.status' does not handle: 'cancelled'"
    );

    // IN list syntax
    let sql_in =
        "SELECT CASE WHEN status IN ('pending', 'paid', 'shipped') THEN 'OK' END FROM orders";
    let diags_in = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql_in);
    assert_eq!(diags_in.len(), 1, "{diags_in:?}");
    assert_eq!(
        diags_in[0].message,
        "CASE over enum column 'orders.status' does not handle: 'cancelled'"
    );

    // OR expression
    let sql_or = "SELECT CASE WHEN status = 'pending' OR status = 'paid' THEN 'OK' WHEN status = 'shipped' THEN 'On its way' END FROM orders";
    let diags_or = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql_or);
    assert_eq!(diags_or.len(), 1, "{diags_or:?}");
    assert_eq!(
        diags_or[0].message,
        "CASE over enum column 'orders.status' does not handle: 'cancelled'"
    );
}

#[test]
fn e0034_silent_when_exhaustive_or_has_else() {
    for sql in [
        // Has ELSE clause
        "SELECT CASE status WHEN 'pending' THEN 'Waiting' ELSE 'Other' END FROM orders",
        "SELECT CASE WHEN status = 'pending' THEN 'Waiting' ELSE 'Other' END FROM orders",
        // All labels handled in simple CASE
        "SELECT CASE status WHEN 'pending' THEN '1' WHEN 'paid' THEN '2' WHEN 'shipped' THEN '3' WHEN 'cancelled' THEN '4' END FROM orders",
        // All labels handled in searched CASE
        "SELECT CASE WHEN status = 'pending' THEN '1' WHEN status = 'paid' THEN '2' WHEN status = 'shipped' THEN '3' WHEN status = 'cancelled' THEN '4' END FROM orders",
        // All labels in IN list
        "SELECT CASE WHEN status IN ('pending', 'paid', 'shipped', 'cancelled') THEN 'ALL' END FROM orders",
        // Non-enum column
        "SELECT CASE id WHEN 1 THEN 'one' WHEN 2 THEN 'two' END FROM orders",
        // General searched CASE comparing different columns
        "SELECT CASE WHEN status = 'pending' THEN '1' WHEN id = 2 THEN '2' END FROM orders",
    ] {
        assert_eq!(codes(sql), Vec::<&str>::new(), "{sql}");
    }
}

#[test]
fn e0034_casts_and_escaped_strings() {
    // Explicit casts: 'pending'::order_status and CAST('paid' AS order_status)
    let sql_casts = "SELECT CASE status
        WHEN 'pending'::order_status THEN '1'
        WHEN CAST('paid' AS order_status) THEN '2'
        WHEN 'shipped'::order_status THEN '3'
        WHEN 'cancelled'::order_status THEN '4'
    END FROM orders";
    assert_eq!(codes(sql_casts), Vec::<&str>::new());

    // Escaped string literal E'...' (Postgres extension)
    let sql_escaped = "SELECT CASE status
        WHEN E'pending' THEN '1'
        WHEN E'paid' THEN '2'
        WHEN E'shipped' THEN '3'
        WHEN E'cancelled' THEN '4'
    END FROM orders";
    assert_eq!(codes(sql_escaped), Vec::<&str>::new());

    // Casted literals in searched CASE
    let sql_searched_cast = "SELECT CASE
        WHEN status = 'pending'::order_status THEN '1'
        WHEN status = E'paid' THEN '2'
        WHEN status = 'shipped' THEN '3'
        WHEN status = 'cancelled' THEN '4'
    END FROM orders";
    assert_eq!(codes(sql_searched_cast), Vec::<&str>::new());

    // Partial coverage recognizes casted label as handled
    let sql_partial = "SELECT CASE status WHEN 'pending'::order_status THEN '1' END FROM orders";
    let diags = diagnostics_with(
        SqlDialect::PostgreSQL,
        PG_SCHEMA,
        pedantic_rules(),
        sql_partial,
    );
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0034");
    assert_eq!(
        diags[0].message,
        "CASE over enum column 'orders.status' does not handle: 'paid', 'shipped', 'cancelled'"
    );
}

#[test]
fn e0034_silent_on_non_literal_when() {
    // Simple CASE with placeholder WHEN $1
    assert_eq!(
        codes("SELECT CASE status WHEN 'pending' THEN '1' WHEN $1 THEN '2' END FROM orders"),
        Vec::<&str>::new()
    );

    // Searched CASE with placeholder WHEN status = $1
    assert_eq!(
        codes("SELECT CASE WHEN status = 'pending' THEN '1' WHEN status = $1 THEN '2' END FROM orders"),
        Vec::<&str>::new()
    );

    // Searched CASE comparing with another column WHEN status = other_col
    assert_eq!(
        codes("SELECT CASE WHEN status = 'pending' THEN '1' WHEN status = id THEN '2' END FROM orders"),
        Vec::<&str>::new()
    );

    // Searched CASE with placeholder in IN list
    assert_eq!(
        codes("SELECT CASE WHEN status IN ('pending', $1) THEN '1' END FROM orders"),
        Vec::<&str>::new()
    );

    // Searched CASE with placeholder in OR branch
    assert_eq!(
        codes("SELECT CASE WHEN status = 'pending' OR status = $1 THEN '1' END FROM orders"),
        Vec::<&str>::new()
    );
}

#[test]
fn e0034_mysql_inline_enum_and_case_insensitivity() {
    // Exhaustive case-insensitive matching
    let sql_ci = "SELECT CASE status
        WHEN 'PENDING' THEN '1'
        WHEN 'PAID' THEN '2'
        WHEN 'SHIPPED' THEN '3'
        WHEN 'CANCELLED' THEN '4'
    END FROM orders";
    let diags_ci = diagnostics_with(SqlDialect::MySQL, MYSQL_SCHEMA, pedantic_rules(), sql_ci);
    assert!(
        !diags_ci.iter().any(|d| d.code() == "E0034"),
        "E0034 should be silent when all labels match case-insensitively on MySQL: {diags_ci:?}"
    );

    // Searched CASE case-insensitive matching
    let sql_ci_searched = "SELECT CASE
        WHEN status = 'PENDING' THEN '1'
        WHEN status = 'PAID' THEN '2'
        WHEN status = 'SHIPPED' THEN '3'
        WHEN status = 'CANCELLED' THEN '4'
    END FROM orders";
    let diags_ci_searched = diagnostics_with(
        SqlDialect::MySQL,
        MYSQL_SCHEMA,
        pedantic_rules(),
        sql_ci_searched,
    );
    assert!(
        !diags_ci_searched.iter().any(|d| d.code() == "E0034"),
        "E0034 should be silent for searched CASE on MySQL: {diags_ci_searched:?}"
    );

    // 1-based enum indexing in simple CASE
    let sql_idx = "SELECT CASE status
        WHEN 1 THEN '1'
        WHEN 2 THEN '2'
        WHEN 3 THEN '3'
        WHEN 4 THEN '4'
    END FROM orders";
    let diags_idx = diagnostics_with(SqlDialect::MySQL, MYSQL_SCHEMA, pedantic_rules(), sql_idx);
    assert!(
        !diags_idx.iter().any(|d| d.code() == "E0034"),
        "E0034 should be silent when matching by 1-based index on MySQL: {diags_idx:?}"
    );

    // 1-based enum indexing in searched CASE
    let sql_idx_searched = "SELECT CASE
        WHEN status = 1 THEN '1'
        WHEN status = 2 THEN '2'
        WHEN status = 3 THEN '3'
        WHEN status = 4 THEN '4'
    END FROM orders";
    let diags_idx_searched = diagnostics_with(
        SqlDialect::MySQL,
        MYSQL_SCHEMA,
        pedantic_rules(),
        sql_idx_searched,
    );
    assert!(
        !diags_idx_searched.iter().any(|d| d.code() == "E0034"),
        "E0034 should be silent for searched CASE by index on MySQL: {diags_idx_searched:?}"
    );

    // Partial coverage reports missing labels using schema casing
    let sql_partial =
        "SELECT CASE status WHEN 'PENDING' THEN 'Waiting' WHEN 2 THEN 'Paid' END FROM orders";
    let diags: Vec<_> = diagnostics_with(
        SqlDialect::MySQL,
        MYSQL_SCHEMA,
        pedantic_rules(),
        sql_partial,
    )
    .into_iter()
    .filter(|d| d.code() == "E0034")
    .collect();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0034");
    assert_eq!(
        diags[0].message,
        "CASE over enum column 'orders.status' does not handle: 'shipped', 'cancelled'"
    );
}

#[test]
fn e0034_silent_in_aggregate_arguments() {
    for sql in [
        "SELECT count(CASE WHEN status = 'paid' THEN 1 END) FROM orders",
        "SELECT count(CASE status WHEN 'paid' THEN 1 END) FROM orders",
        "SELECT sum(CASE WHEN status = 'paid' THEN id END) FROM orders",
        "SELECT avg(CASE WHEN status = 'paid' THEN id END) FROM orders",
        "SELECT min(CASE WHEN status = 'paid' THEN id END) FROM orders",
        "SELECT max(CASE WHEN status = 'paid' THEN id END) FROM orders",
        "SELECT array_agg(CASE WHEN status = 'paid' THEN id END) FROM orders",
        "SELECT string_agg(CASE WHEN status = 'paid' THEN 'x' END, ',') FROM orders",
        "SELECT bool_or(CASE WHEN status = 'paid' THEN true END) FROM orders",
        "SELECT bool_and(CASE WHEN status = 'paid' THEN true END) FROM orders",
    ] {
        assert_eq!(codes(sql), Vec::<&str>::new(), "{sql}");
    }

    // Conditional aggregate does not silence sibling non-aggregate CASE
    let sql_mixed = "SELECT count(CASE WHEN status = 'paid' THEN 1 END), CASE status WHEN 'pending' THEN 1 END FROM orders";
    let diags: Vec<_> = diagnostics_with(
        SqlDialect::PostgreSQL,
        PG_SCHEMA,
        pedantic_rules(),
        sql_mixed,
    )
    .into_iter()
    .filter(|d| d.code() == "E0034")
    .collect();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0034");
    assert_eq!(
        diags[0].message,
        "CASE over enum column 'orders.status' does not handle: 'paid', 'shipped', 'cancelled'"
    );
}

#[test]
fn e0034_single_rule_can_be_enabled() {
    let mut rules = RuleConfig::default();
    rules
        .configure("case-enum-missing-labels", RuleLevel::Error)
        .unwrap();
    let sql = "SELECT CASE status WHEN 'pending' THEN 'Waiting' END FROM orders";
    let diags = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, rules, sql);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0034");
    assert_eq!(diags[0].severity, Severity::Error);
}
