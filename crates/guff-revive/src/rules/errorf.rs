//! `errorf` — prefer `fmt.Errorf` over `errors.New(fmt.Sprintf(...))`.

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_pkg_dot_name, type_of};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
                    let NodeRef::CallExpr(call) = n else { return; };
                    check_call(self.pass, call, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}


fn check_call(pass: &Pass<'_>, call: &CallExpr, failures: &mut Vec<Failure>) {
    if call.args.len() != 1 {
        return;
    }
    let is_errors_new = is_pkg_dot_name(&call.fun, "errors", "New");
    let mut prefix = "fmt".to_string();
    let mut render_target = "errors.New".to_string();
    // Plain matches, not `unparen`: upstream writes `ce.Fun.(*ast.SelectorExpr)`
    // and `arg.(*ast.CallExpr)`, so `errors.New((fmt.Sprintf(…)))` is a shape it
    // stays silent on. See the note in range_val_address.rs — same class, found
    // the same way (compat/fuzz.py, COMPAT-HARDENING Phase 6).
    let is_testing_error = if let Expr::SelectorExpr(sel) = &*call.fun {
        if sel.sel.name == "Error" {
            if let Some(typ) = type_of(pass, &sel.x) {
                let s = crate::util::type_string(pass, typ);
                if s == "*testing.T" {
                    // Upstream: `w.file.Render(se.X)` — the source text, not
                    // the identifier, so a receiver that is not a bare name
                    // still renders as written instead of the fixed word "t".
                    prefix = {
                        let mut buf: Vec<u8> = Vec::new();
                        match guff::printer::fprint(
                            &mut buf,
                            pass.fset(),
                            guff::printer::PrintNode::Expr(&sel.x),
                        )
                        .ok()
                        .and_then(|_| String::from_utf8(buf).ok())
                        {
                            Some(text) => text,
                            None => "t".into(),
                        }
                    };
                    render_target = format!("{prefix}.Error");
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    if !is_errors_new && !is_testing_error {
        return;
    }
    let Expr::CallExpr(inner) = &call.args[0] else {
        return;
    };
    if !is_pkg_dot_name(&inner.fun, "fmt", "Sprintf") {
        return;
    }
    // Upstream rebuilds the line with a regex over the *source text*, not from
    // the AST: `^(.*)<rendered selector>\(fmt\.Sprintf\((.*)\)\)(.*)$`, then
    // splices `prefix + ".Errorf(" + args + ")"` between the outer groups. The
    // selector goes in unescaped, exactly as upstream writes it — escaping it
    // would be more correct and less faithful, and `.` matching one character
    // cannot change the answer for a real package name.
    //
    // No match means no fix, which is upstream's behaviour too: a call split
    // across lines reports and rewrites nothing.
    let replacement_line = crate::util::src_line_at(pass, inner.fun.pos().0 as u32).and_then(|line| {
        let pattern = format!(r"^(.*){render_target}\(fmt\.Sprintf\((.*)\)\)(.*)$");
        let re = regex::Regex::new(&pattern).ok()?;
        let c = re.captures(&line)?;
        Some(format!(
            "{}{prefix}.Errorf({}){}",
            c.get(1)?.as_str(),
            c.get(2)?.as_str(),
            c.get(3)?.as_str()
        ))
    });
    failures.push(Failure {
        rule: "errorf",
        pos: call.fun.pos().0 as u32,
        message: format!(
            "should replace {render_target}(fmt.Sprintf(...)) with {prefix}.Errorf(...)"
        ),
        replacement_line,
        replacement_end: Some(call.end().0 as u32),
        ..Failure::default()
    });
}
