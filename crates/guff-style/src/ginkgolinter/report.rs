//! `internal/reports` and `internal/formatter`.

use guff::ast::{CallExpr, Expr};
use guff_analysis::{Diagnostic, SuggestedFix, TextEdit};

use super::types::Tc;

/// `formatter.GoFmtFormatter.Format`: `printer.Fprint` of the expression.
pub(super) fn format(tc: &Tc<'_>, e: &Expr) -> String {
    let mut buf: Vec<u8> = Vec::new();
    if guff::printer::fprint(&mut buf, tc.fset(), guff::printer::PrintNode::Expr(e)).is_err() {
        return String::new();
    }
    String::from_utf8(buf).unwrap_or_default()
}

/// `reports.Builder`.
pub(super) struct Builder {
    pos: u32,
    end: u32,
    old_expr: String,
    issues: Vec<String>,
    fix_offer: String,
    suggest_fix: bool,
}

impl Builder {
    pub fn new(tc: &Tc<'_>, old: &CallExpr) -> Builder {
        let e = Expr::CallExpr(old.clone());
        Builder {
            pos: old.pos().0 as u32,
            end: old.end().0 as u32,
            old_expr: format(tc, &e),
            issues: Vec::new(),
            fix_offer: String::new(),
            suggest_fix: false,
        }
    }

    pub fn add_issue(&mut self, suggest_fix: bool, issue: String) {
        self.issues.push(issue);
        if suggest_fix {
            self.suggest_fix = true;
        }
    }

    pub fn format_expr(&self, tc: &Tc<'_>, e: &Expr) -> String {
        format(tc, e)
    }

    pub fn has_report(&self) -> bool {
        !self.issues.is_empty()
    }

    pub fn set_fix_offer(&mut self, tc: &Tc<'_>, offer: Option<&CallExpr>) {
        if !self.suggest_fix {
            return;
        }
        let Some(offer) = offer else {
            return;
        };
        let text = format(tc, &Expr::CallExpr(offer.clone()));
        if text != self.old_expr {
            self.fix_offer = text;
        }
    }

    pub fn build(&self) -> Diagnostic {
        let mut message = String::from("ginkgo-linter: ");
        if self.issues.len() > 1 {
            message.push_str("multiple issues: ");
        }
        message.push_str(&self.issues.join("; "));
        let fix = self.suggest_fix && !self.fix_offer.is_empty();
        if fix {
            message.push_str(&format!(". Consider using `{}` instead", self.fix_offer));
        }
        let suggested_fixes = if fix {
            vec![SuggestedFix {
                message: format!("should replace {} with {}", self.old_expr, self.fix_offer),
                text_edits: vec![TextEdit {
                    pos: self.pos,
                    end: self.end,
                    new_text: self.fix_offer.clone(),
                }],
            }]
        } else {
            Vec::new()
        };
        Diagnostic {
            pos: self.pos,
            message,
            suggested_fixes,
            ..Diagnostic::default()
        }
    }
}
