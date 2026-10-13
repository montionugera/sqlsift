//! Pedantic rules (off by default): stricter checks that may have false positives
//!
//! - E0033 case-enum-missing-labels: `CASE` over an enum column that misses some
//!   labels and has no `ELSE` clause

use sqlsift_core::dialect::SqlDialect;
use sqlsift_core::error::Severity;
use sqlsift_core::rules::{RuleConfig, RuleLevel};
use sqlsift_core::schema::SchemaBuilder;
use sqlsift_core::Analyzer;

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

#[test]
fn off_by_default() {
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
fn e0033_simple_case_missing_labels() {
    let sql = "SELECT CASE status WHEN 'pending' THEN 'Waiting' WHEN 'paid' THEN 'Paid' WHEN 'shipped' THEN 'On its way' END FROM orders";
    let diags = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0033");
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
fn e0033_searched_case_missing_labels() {
    let sql = "SELECT CASE WHEN status = 'pending' THEN 'Waiting' WHEN status = 'paid' THEN 'Paid' WHEN status = 'shipped' THEN 'On its way' END FROM orders";
    let diags = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, pedantic_rules(), sql);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0033");
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
fn e0033_silent_when_exhaustive_or_has_else() {
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
fn e0033_mysql_inline_enum() {
    let sql =
        "SELECT CASE status WHEN 'pending' THEN 'Waiting' WHEN 'paid' THEN 'Paid' END FROM orders";
    let diags = diagnostics_with(SqlDialect::MySQL, MYSQL_SCHEMA, pedantic_rules(), sql);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code(), "E0033");
    assert_eq!(
        diags[0].message,
        "CASE over enum column 'orders.status' does not handle: 'shipped', 'cancelled'"
    );
}

#[test]
fn single_rule_can_be_enabled() {
    let mut rules = RuleConfig::default();
    rules
        .configure("case-enum-missing-labels", RuleLevel::Error)
        .unwrap();
    let sql = "SELECT CASE status WHEN 'pending' THEN 'Waiting' END FROM orders";
    let diags = diagnostics_with(SqlDialect::PostgreSQL, PG_SCHEMA, rules, sql);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0033");
    assert_eq!(diags[0].severity, Severity::Error);
}
