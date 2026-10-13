//! Pedantic rules (off by default): integer division (E0032).

use sqlsift_core::dialect::SqlDialect;
use sqlsift_core::rules::{RuleConfig, RuleLevel};
use sqlsift_core::schema::SchemaBuilder;
use sqlsift_core::Analyzer;

const SCHEMA: &str = "
CREATE TABLE orders (id INT PRIMARY KEY, qty INT, items INT, price NUMERIC);
";

fn diagnostics_in(
    dialect: SqlDialect,
    rules: RuleConfig,
    sql: &str,
) -> Vec<sqlsift_core::Diagnostic> {
    let mut builder = SchemaBuilder::with_dialect(dialect);
    builder.parse(SCHEMA).expect("schema parses");
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

fn pedantic_rules() -> RuleConfig {
    let mut rules = RuleConfig::default();
    rules.configure("pedantic", RuleLevel::Warn).unwrap();
    rules
}

fn codes(sql: &str) -> Vec<&'static str> {
    codes_in(SqlDialect::PostgreSQL, pedantic_rules(), sql)
}

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
        "Integer division truncates toward zero: integer / integer produces an integer"
    );
    assert_eq!(
        diags[0].help.as_deref(),
        Some(
            "Cast an operand to NUMERIC or FLOAT if a ratio or average is meant: CAST(x AS NUMERIC) / y"
        )
    );
    let span = diags[0].span.unwrap();
    assert_eq!(span.line, 1);
    assert_eq!(span.column, 8);
    assert_eq!(span.length, "qty / items".len());
}

#[test]
fn e0032_column_and_literal() {
    assert_eq!(codes("SELECT qty / 3 FROM orders"), ["E0032"]);
    assert_eq!(codes("SELECT 10 / items FROM orders"), ["E0032"]);
}

#[test]
fn e0032_two_literals_are_clean() {
    assert!(codes("SELECT 10 / 3").is_empty());
    assert!(codes("SELECT 10 / 3 FROM orders").is_empty());
    assert!(codes("SELECT -10 / 3").is_empty());
    assert!(codes("SELECT (10 + 2) / 3").is_empty());
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
        "Integer division truncates toward zero: bigint / bigint produces an integer"
    );
    let span = diags[0].span.unwrap();
    assert_eq!(span.line, 1);
    assert_eq!(span.column, 8);
    assert_eq!(span.length, 28);

    let diags_count = diagnostics_in(
        SqlDialect::PostgreSQL,
        pedantic_rules(),
        "SELECT count(*) / 10 FROM orders",
    );
    assert_eq!(diags_count.len(), 1);
    let span_count = diags_count[0].span.unwrap();
    assert_eq!(span_count.line, 1);
    assert_eq!(span_count.column, 8);
    assert_eq!(span_count.length, "count(*) / 10".len());

    assert_eq!(codes("SELECT count(*) / 10 FROM orders"), ["E0032"]);
    assert_eq!(codes("SELECT 100 / count(*) FROM orders"), ["E0032"]);
}

#[test]
fn e0032_function_call() {
    let diags = diagnostics_in(
        SqlDialect::PostgreSQL,
        pedantic_rules(),
        "SELECT length('test') / 2",
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), "E0032");
    let span = diags[0].span.unwrap();
    assert_eq!(span.line, 1);
    assert_eq!(span.column, 8);
    assert_eq!(span.length, "length('test') / 2".len());
}

#[test]
fn e0032_numeric_or_cast_is_clean() {
    assert!(codes("SELECT qty::numeric / items FROM orders").is_empty());
    assert!(codes("SELECT CAST(qty AS NUMERIC) / items FROM orders").is_empty());
    assert!(codes("SELECT qty / CAST(items AS FLOAT) FROM orders").is_empty());
    assert!(codes("SELECT price / items FROM orders").is_empty());
    assert!(codes("SELECT (1.0 * qty) / items FROM orders").is_empty());
}

#[test]
fn e0032_inner_division_in_cast_is_reported() {
    // Truncates before the cast to numeric
    assert_eq!(
        codes("SELECT (qty / items)::numeric FROM orders"),
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
