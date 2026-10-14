//! Pedantic rules (opt-in, off by default): stricter checks that may have false positives
//!
//! - E0034 case-enum-missing-labels: `CASE` over an enum column that misses some
//!   labels and has no `ELSE` clause

use std::collections::HashSet;

use sqlparser::ast::{BinaryOperator, CaseWhen, Expr, Value, ValueWithSpan};

use crate::dialect::SqlDialect;
use crate::error::{Diagnostic, DiagnosticKind};
use crate::types::SqlType;

use super::resolver::Resolver;
use super::scope::ExpressionType;
use super::type_check::located_span;

enum LabelMatch {
    Covered(usize),
    OtherLiteral,
    NonLiteral,
}

impl Resolver<'_> {
    /// E0034: CASE over an enum column that misses some labels and has no ELSE
    pub(super) fn check_case_enum_missing_labels(
        &mut self,
        case_expr: &Expr,
        operand: Option<&Expr>,
        conditions: &[CaseWhen],
        else_result: Option<&Expr>,
    ) {
        // If the rule is off, or inside an aggregate argument (where NULL is the point
        // of conditional aggregation like count(CASE ...)), or an ELSE clause is present:
        if !self.case_enum_missing_labels
            || self.in_aggregate_arg > 0
            || else_result.is_some()
            || conditions.is_empty()
        {
            return;
        }

        let (col_expr, values, handled) = if let Some(op) = operand {
            let op_unnested = unnest_expr(op);
            let ExpressionType::Known(ty) = self.infer_expr_type(op_unnested) else {
                return;
            };
            let values = match ty {
                SqlType::Custom(ref name) => match self.catalog.get_enum(name) {
                    Some(def) => def.values.clone(),
                    None => return,
                },
                SqlType::Enum(ref values) => values.clone(),
                _ => return,
            };
            if values.is_empty() {
                return;
            }
            let mut handled = HashSet::new();
            for when in conditions {
                match classify_label_expr(&when.condition, self.dialect, &values) {
                    LabelMatch::Covered(idx) => {
                        handled.insert(idx);
                    }
                    LabelMatch::OtherLiteral => {}
                    LabelMatch::NonLiteral => {
                        // A WHEN branch that isn't a recognized label literal (e.g. WHEN $1)
                        // means we cannot statically tell what labels this branch covers.
                        // Bail out and stay silent.
                        return;
                    }
                }
            }
            (op_unnested, values, handled)
        } else {
            let Some((col, values, handled)) = self.searched_case_target(conditions) else {
                return;
            };
            (col, values, handled)
        };

        if handled.is_empty() {
            return;
        }

        let missing: Vec<&str> = values
            .iter()
            .enumerate()
            .filter(|(i, _)| !handled.contains(i))
            .map(|(_, v)| v.as_str())
            .collect();

        if missing.is_empty() {
            return;
        }

        let missing_str = missing
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<_>>()
            .join(", ");

        let col_name = self
            .column_label(col_expr)
            .unwrap_or_else(|| match col_expr {
                Expr::Identifier(ident) => ident.value.clone(),
                Expr::CompoundIdentifier(parts) => parts
                    .iter()
                    .map(|p| p.value.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
                _ => "column".to_string(),
            });

        let mut diag = Diagnostic::warning(
            DiagnosticKind::CaseEnumMissingLabels,
            format!("CASE over enum column '{col_name}' does not handle: {missing_str}"),
        )
        .with_help("Add WHEN branches for the missing labels or an ELSE clause to handle them.");
        diag.span = located_span(case_expr);
        self.diagnostics.push(diag);
    }

    fn searched_case_target<'a>(
        &self,
        conditions: &'a [CaseWhen],
    ) -> Option<(&'a Expr, Vec<String>, HashSet<usize>)> {
        let first = conditions.first()?;
        let (first_col, first_vals) = extract_column_comparisons(&first.condition)?;

        let ExpressionType::Known(ty) = self.infer_expr_type(first_col) else {
            return None;
        };

        let values = match ty {
            SqlType::Custom(ref name) => self.catalog.get_enum(name)?.values.clone(),
            SqlType::Enum(ref values) => values.clone(),
            _ => return None,
        };

        if values.is_empty() {
            return None;
        }

        let mut handled = HashSet::new();

        for val in first_vals {
            match classify_label_expr(val, self.dialect, &values) {
                LabelMatch::Covered(idx) => {
                    handled.insert(idx);
                }
                LabelMatch::OtherLiteral => {}
                LabelMatch::NonLiteral => return None,
            }
        }

        // Check if every condition compares the same column
        for when in &conditions[1..] {
            let (col, vals) = extract_column_comparisons(&when.condition)?;
            if !same_column(first_col, col) {
                return None;
            }
            for val in vals {
                match classify_label_expr(val, self.dialect, &values) {
                    LabelMatch::Covered(idx) => {
                        handled.insert(idx);
                    }
                    LabelMatch::OtherLiteral => {}
                    LabelMatch::NonLiteral => return None,
                }
            }
        }

        Some((first_col, values, handled))
    }
}

fn classify_label_expr(expr: &Expr, dialect: SqlDialect, enum_values: &[String]) -> LabelMatch {
    let unnested = unnest_label_expr(expr);
    let value = match unnested {
        Expr::Value(ValueWithSpan { value, .. }) => value,
        Expr::TypedString(ts) => &ts.value.value,
        _ => return LabelMatch::NonLiteral,
    };
    if let Some(s) = value_as_string(value) {
        if dialect == SqlDialect::MySQL {
            if let Some((idx, _)) = enum_values
                .iter()
                .enumerate()
                .find(|(_, v)| v.eq_ignore_ascii_case(s))
            {
                return LabelMatch::Covered(idx);
            }
        } else if let Some((idx, _)) = enum_values
            .iter()
            .enumerate()
            .find(|(_, v)| v.as_str() == s)
        {
            return LabelMatch::Covered(idx);
        }
        return LabelMatch::OtherLiteral;
    }
    if dialect == SqlDialect::MySQL {
        if let Value::Number(n, _) = value {
            if let Ok(idx) = n.parse::<usize>() {
                if idx >= 1 && idx <= enum_values.len() {
                    return LabelMatch::Covered(idx - 1);
                }
                return LabelMatch::OtherLiteral;
            }
        }
    }
    if matches!(value, Value::Null) {
        return LabelMatch::OtherLiteral;
    }
    LabelMatch::NonLiteral
}

fn unnest_label_expr(mut expr: &Expr) -> &Expr {
    loop {
        match expr {
            Expr::Nested(inner) => expr = inner,
            Expr::Cast { expr: inner, .. } => expr = inner,
            _ => break expr,
        }
    }
}

fn value_as_string(value: &Value) -> Option<&str> {
    match value {
        Value::SingleQuotedString(s)
        | Value::DoubleQuotedString(s)
        | Value::EscapedStringLiteral(s)
        | Value::NationalStringLiteral(s)
        | Value::UnicodeStringLiteral(s)
        | Value::TripleSingleQuotedString(s)
        | Value::TripleDoubleQuotedString(s)
        | Value::SingleQuotedRawStringLiteral(s)
        | Value::DoubleQuotedRawStringLiteral(s) => Some(s.as_str()),
        Value::DollarQuotedString(d) => Some(d.value.as_str()),
        _ => None,
    }
}

fn extract_column_comparisons(expr: &Expr) -> Option<(&Expr, Vec<&Expr>)> {
    match unnest_expr(expr) {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            if is_column_expr(left) {
                Some((left, vec![right.as_ref()]))
            } else if is_column_expr(right) {
                Some((right, vec![left.as_ref()]))
            } else {
                None
            }
        }
        Expr::InList {
            expr,
            list,
            negated: false,
        } => {
            if !is_column_expr(expr) || list.is_empty() {
                None
            } else {
                Some((expr, list.iter().collect()))
            }
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            let (col_l, mut vals_l) = extract_column_comparisons(left)?;
            let (col_r, vals_r) = extract_column_comparisons(right)?;
            if !same_column(col_l, col_r) {
                return None;
            }
            vals_l.extend(vals_r);
            Some((col_l, vals_l))
        }
        _ => None,
    }
}

fn is_column_expr(expr: &Expr) -> bool {
    matches!(
        unnest_expr(expr),
        Expr::Identifier(_) | Expr::CompoundIdentifier(_)
    )
}

fn same_column(a: &Expr, b: &Expr) -> bool {
    let a = unnest_expr(a);
    let b = unnest_expr(b);
    match (a, b) {
        (Expr::Identifier(ia), Expr::Identifier(ib)) => ia.value == ib.value,
        (Expr::CompoundIdentifier(pa), Expr::CompoundIdentifier(pb)) => {
            pa.len() == pb.len() && pa.iter().zip(pb).all(|(x, y)| x.value == y.value)
        }
        _ => false,
    }
}

fn unnest_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::Nested(inner) => unnest_expr(inner),
        _ => expr,
    }
}
