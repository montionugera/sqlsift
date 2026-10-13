//! Pedantic rules (opt-in, off by default): stricter checks that may have false positives
//!
//! - E0033 case-enum-missing-labels: `CASE` over an enum column that misses some
//!   labels and has no `ELSE` clause

use std::collections::HashSet;

use sqlparser::ast::{BinaryOperator, CaseWhen, Expr, Value, ValueWithSpan};

use crate::error::{Diagnostic, DiagnosticKind};
use crate::types::SqlType;

use super::resolver::Resolver;
use super::scope::ExpressionType;
use super::type_check::located_span;

impl Resolver<'_> {
    /// E0033: CASE over an enum column that misses some labels and has no ELSE
    pub(super) fn check_case_enum_missing_labels(
        &mut self,
        case_expr: &Expr,
        operand: Option<&Expr>,
        conditions: &[CaseWhen],
        else_result: Option<&Expr>,
    ) {
        // If an ELSE clause is present, any unhandled label falls into ELSE.
        if else_result.is_some() || conditions.is_empty() {
            return;
        }

        let (col_expr, values) = if let Some(op) = operand {
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
            (op_unnested, values)
        } else {
            let Some((col, values)) = self.searched_case_target(conditions) else {
                return;
            };
            (col, values)
        };

        if values.is_empty() {
            return;
        }

        let mut handled = HashSet::new();
        if operand.is_some() {
            for when in conditions {
                if let Some(s) = string_literal(&when.condition) {
                    handled.insert(s);
                }
            }
        } else {
            for when in conditions {
                if let Some((_, labels)) = extract_col_and_labels(&when.condition) {
                    for label in labels {
                        handled.insert(label);
                    }
                }
            }
        }

        if handled.is_empty() {
            return;
        }

        let missing: Vec<&str> = values
            .iter()
            .map(String::as_str)
            .filter(|v| !handled.contains(*v))
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
    ) -> Option<(&'a Expr, Vec<String>)> {
        let first = conditions.first()?;
        let (first_col, _) = extract_col_and_labels(&first.condition)?;

        // Check if every condition compares the same column
        for when in &conditions[1..] {
            let (col, _) = extract_col_and_labels(&when.condition)?;
            if !same_column(first_col, col) {
                return None;
            }
        }

        let ExpressionType::Known(ty) = self.infer_expr_type(first_col) else {
            return None;
        };

        let values = match ty {
            SqlType::Custom(ref name) => self.catalog.get_enum(name)?.values.clone(),
            SqlType::Enum(ref values) => values.clone(),
            _ => return None,
        };

        Some((first_col, values))
    }
}

fn string_literal(expr: &Expr) -> Option<&str> {
    match unnest_expr(expr) {
        Expr::Value(ValueWithSpan {
            value: Value::SingleQuotedString(s) | Value::DoubleQuotedString(s),
            ..
        }) => Some(s.as_str()),
        _ => None,
    }
}

fn extract_col_and_labels(expr: &Expr) -> Option<(&Expr, Vec<&str>)> {
    match unnest_expr(expr) {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            if let Some(s) = string_literal(right) {
                if is_column_expr(left) {
                    return Some((left, vec![s]));
                }
            }
            if let Some(s) = string_literal(left) {
                if is_column_expr(right) {
                    return Some((right, vec![s]));
                }
            }
            None
        }
        Expr::InList {
            expr,
            list,
            negated: false,
        } => {
            if !is_column_expr(expr) || list.is_empty() {
                return None;
            }
            let mut labels = Vec::with_capacity(list.len());
            for item in list {
                let s = string_literal(item)?;
                labels.push(s);
            }
            Some((expr, labels))
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            let (col_l, mut labels_l) = extract_col_and_labels(left)?;
            let (col_r, labels_r) = extract_col_and_labels(right)?;
            if !same_column(col_l, col_r) {
                return None;
            }
            labels_l.extend(labels_r);
            Some((col_l, labels_l))
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
