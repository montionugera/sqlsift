//! E0027 wrong-argument-type: built-in functions and operators given an argument
//! type PostgreSQL has no version for. Every query reported here fails on
//! PostgreSQL 16 and every query asserted clean runs.

use sqlsift_core::dialect::SqlDialect;
use sqlsift_core::schema::SchemaBuilder;
use sqlsift_core::Analyzer;

const SCHEMA: &str = "
CREATE TABLE s (
    id INT, n NUMERIC, f FLOAT8, b BOOLEAN, d DATE, ts TIMESTAMP, u UUID, j JSONB,
    name TEXT, v VARCHAR(5), c CHAR(3), iv INTERVAL, by BYTEA
);
";

fn codes_with(schema: &str, dialect: SqlDialect, sql: &str) -> Vec<&'static str> {
    let mut builder = SchemaBuilder::with_dialect(dialect);
    builder.parse(schema).expect("schema parses");
    let catalog = builder.build().0;
    Analyzer::with_dialect(&catalog, dialect)
        .analyze(sql)
        .iter()
        .map(sqlsift_core::Diagnostic::code)
        .collect()
}

fn codes(sql: &str) -> Vec<&'static str> {
    codes_with(SCHEMA, SqlDialect::PostgreSQL, sql)
}

#[test]
fn aggregates_of_non_numbers() {
    for sql in [
        "SELECT sum(name) FROM s",
        "SELECT sum(b) FROM s",
        "SELECT sum(d) FROM s",
        "SELECT avg(name) FROM s",
        "SELECT avg(ts) FROM s",
        "SELECT sum(u) FROM s",
        "SELECT pg_catalog.sum(j) FROM s",
    ] {
        assert_eq!(codes(sql), ["E0027"], "{sql}");
    }
}

#[test]
fn text_functions_of_non_text() {
    for sql in [
        "SELECT lower(id) FROM s",
        "SELECT upper(n) FROM s",
        "SELECT length(id) FROM s",
        "SELECT length(d) FROM s",
        "SELECT char_length(b) FROM s",
        "SELECT initcap(u) FROM s",
        "SELECT ltrim(id) FROM s",
        "SELECT reverse(ts) FROM s",
        "SELECT md5(id) FROM s",
        "SELECT lower(j) FROM s",
    ] {
        assert_eq!(codes(sql), ["E0027"], "{sql}");
    }
}

#[test]
fn like_on_non_text() {
    for sql in [
        "SELECT id FROM s WHERE id LIKE '1%'",
        "SELECT id FROM s WHERE d LIKE '2020%'",
        "SELECT id FROM s WHERE b ILIKE 't%'",
        "SELECT id FROM s WHERE u NOT LIKE 'a%'",
        "SELECT id FROM s WHERE n LIKE '1%'",
    ] {
        assert_eq!(codes(sql), ["E0027"], "{sql}");
    }
}

#[test]
fn matching_signatures_are_fine() {
    for sql in [
        "SELECT sum(id), sum(n), sum(f), sum(iv), avg(iv), avg(id) FROM s",
        "SELECT lower(name), lower(v), lower(c), length(by), length(name), md5(by), lower('X') FROM s",
        "SELECT id FROM s WHERE name LIKE 'a%' OR v LIKE 'a%' OR c LIKE 'a%' OR by LIKE 'a%'",
        "SELECT id FROM s WHERE id::text LIKE '1%'",
        "SELECT lower(id::text) FROM s",
        "SELECT sum(name::int) FROM s",
        // Another schema's function is not the built-in one
        "SELECT util.lower(id) FROM s",
    ] {
        assert!(codes(sql).is_empty(), "{sql}: {:?}", codes(sql));
    }
}

#[test]
fn functions_and_operators_the_schema_defines_may_overload() {
    let schema = format!(
        "{SCHEMA}\nCREATE OR REPLACE FUNCTION public.lower(integer) RETURNS text AS $$ SELECT $1::text $$ LANGUAGE sql;\n\
         CREATE AGGREGATE sum(text) (SFUNC = textcat, STYPE = text);"
    );
    let pg = |sql| codes_with(&schema, SqlDialect::PostgreSQL, sql);
    assert!(pg("SELECT lower(id) FROM s").is_empty());
    assert!(pg("SELECT sum(name) FROM s").is_empty());
    assert_eq!(pg("SELECT upper(id) FROM s"), ["E0027"]);

    let schema =
        format!("{SCHEMA}\nCREATE OPERATOR ~~ (LEFTARG = integer, RIGHTARG = text, FUNCTION = f);");
    assert!(codes_with(
        &schema,
        SqlDialect::PostgreSQL,
        "SELECT id FROM s WHERE id LIKE '1%'"
    )
    .is_empty());
}

#[test]
fn schema_defined_function_skips_builtin_return_type() {
    let schema = format!(
        "{SCHEMA}\nCREATE OR REPLACE FUNCTION date_trunc(text, text) RETURNS integer AS $$ SELECT 1 $$ LANGUAGE sql;"
    );
    let pg = |sql| codes_with(&schema, SqlDialect::PostgreSQL, sql);
    assert!(pg("SELECT id FROM s WHERE date_trunc('day', 'x') = 1").is_empty());
}

#[test]
fn other_dialects_convert_implicitly() {
    for dialect in [SqlDialect::MySQL, SqlDialect::SQLite] {
        assert!(codes_with(
            SCHEMA,
            dialect,
            "SELECT lower(id), sum(name) FROM s WHERE id LIKE '1%'"
        )
        .is_empty());
    }
}

#[test]
fn multi_argument_text_functions_reported() {
    for sql in [
        "SELECT lpad(id, 8, '0') FROM s",
        "SELECT rpad(id, 8, '0') FROM s",
        "SELECT left(d, 4) FROM s",
        "SELECT right(d, 4) FROM s",
        "SELECT replace(id, '1', '2') FROM s",
        "SELECT split_part(id, ',', 1) FROM s",
        "SELECT strpos(id, '1') FROM s",
        "SELECT starts_with(id, '1') FROM s",
        "SELECT repeat(id, 3) FROM s",
        "SELECT translate(id, '1', '2') FROM s",
    ] {
        assert_eq!(codes(sql), ["E0027"], "{sql}");
    }
}

#[test]
fn multi_argument_text_functions_not_reported() {
    for sql in [
        "SELECT lpad(name, 8, '0') FROM s",
        "SELECT rpad(name, 8, '0') FROM s",
        "SELECT left(name, 3) FROM s",
        "SELECT right(name, 3) FROM s",
        "SELECT split_part(name, ',', 1) FROM s",
        "SELECT strpos(name, 'a') FROM s",
        "SELECT starts_with(name, 'a') FROM s",
        "SELECT replace(name, '1', '2') FROM s",
        "SELECT repeat(name, 2) FROM s",
        "SELECT translate(name, '1', '2') FROM s",
        "SELECT lpad(id::text, 8, '0') FROM s",
        "SELECT left(name, 3) FROM s",
        "SELECT lpad(name, id) FROM s",
    ] {
        assert!(codes(sql).is_empty(), "{sql}: {:?}", codes(sql));
    }

    for dialect in [SqlDialect::MySQL, SqlDialect::SQLite] {
        assert!(codes_with(
            SCHEMA,
            dialect,
            "SELECT lpad(id, 8, '0'), left(d, 4), replace(id, '1', '2') FROM s"
        )
        .is_empty());
    }
}

#[test]
fn text_functions_parenthesize_complex_expression_in_cast_help() {
    let mut builder = SchemaBuilder::with_dialect(SqlDialect::PostgreSQL);
    builder.parse(SCHEMA).expect("schema parses");
    let catalog = builder.build().0;
    let analyze = |sql| Analyzer::with_dialect(&catalog, SqlDialect::PostgreSQL).analyze(sql);

    let diags = analyze("SELECT lpad(id + 1, 8, '0') FROM s");
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].help.as_deref(),
        Some("Cast the argument to text: lpad((id + 1)::text, 8, '0')")
    );

    let diags = analyze("SELECT lower(id + 1) FROM s");
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].help.as_deref(),
        Some("Cast the argument to text: lower((id + 1)::text)")
    );

    let diags = analyze("SELECT lpad(id, 8, '0') FROM s");
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].help.as_deref(),
        Some("Cast the argument to text: lpad(id::text, 8, '0')")
    );
}

#[test]
fn regression_review_361_builtin_inference_operator_and_function_refinements() {
    let schema_with_hstore = format!(
        "{SCHEMA}\nCREATE TABLE sh (h HSTORE, name TEXT);\nCREATE TABLE flags (flag INT, flag2 INT);\nCREATE TABLE jdata (j JSONB);\n"
    );
    let pg = |sql| codes_with(&schema_with_hstore, SqlDialect::PostgreSQL, sql);
    let mysql = |sql| codes_with(&schema_with_hstore, SqlDialect::MySQL, sql);
    let sqlite = |sql| codes_with(&schema_with_hstore, SqlDialect::SQLite, sql);

    // 1. bytea: b || b = b and string_agg(b, ',') = b (no false positive)
    assert!(pg("SELECT id FROM s WHERE by || by = by").is_empty());
    assert!(pg("SELECT string_agg(by, ',') = by FROM s GROUP BY by").is_empty());
    assert_eq!(pg("SELECT id FROM s WHERE by || by = 1"), ["E0003"]);
    assert_eq!(pg("SELECT string_agg(by, ',') = 1 FROM s"), ["E0003"]);
    assert_eq!(pg("SELECT string_agg(name, ',') = 1 FROM s"), ["E0003"]);

    // 2. array: array_agg(id) || 1 (no false positive)
    assert!(pg("SELECT array_agg(id) || 1 FROM s").is_empty());
    assert!(pg("SELECT (array_agg(id) || 1) = ARRAY[1] FROM s").is_empty());

    // 3. hstore: h -> 'k' = name and h -> 'k' LIKE 'a%' (no false positive)
    assert!(pg("SELECT * FROM sh WHERE h -> 'k' = name").is_empty());
    assert!(pg("SELECT * FROM sh WHERE h -> 'k' LIKE 'a%'").is_empty());
    // For jsonb, -> is jsonb so LIKE is rejected
    assert_eq!(pg("SELECT * FROM s WHERE j -> 'k' LIKE 'a%'"), ["E0027"]);

    // 4. MySQL: flag = 1 || flag2 = 1 (no E0029 false positive)
    assert!(mysql("SELECT * FROM flags WHERE flag = 1 || flag2 = 1").is_empty());

    // 5. SQLite: j->>'$.n' = 1 (no E0003 false positive)
    assert!(sqlite("SELECT * FROM jdata WHERE j->>'$.n' = 1").is_empty());
    // In PostgreSQL, ->> returns text so comparing to int reports E0003
    assert_eq!(pg("SELECT * FROM jdata WHERE j->>'$.n' = 1"), ["E0003"]);

    // 6. date_trunc with unknown argument stays Unknown (no false positive)
    assert!(pg("SELECT id FROM s WHERE date_trunc('day', unknown_fn(ts)) = 1").is_empty());
    // date_trunc with known timestamp column compares to int -> E0003
    assert_eq!(
        pg("SELECT id FROM s WHERE date_trunc('day', ts) = 1"),
        ["E0003"]
    );
}
