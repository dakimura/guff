mod support;

use guff_error::{durationcheck, err113, errchkjson, errname, errorlint, rowserrcheck, wrapcheck};

#[test]
fn errname_flags_bad_type_and_var_names() {
    let dir = support::testdata("errname");
    let pkg = support::typecheck_pkg("example.com/errname", &dir.join("bad.go"));
    let messages = support::run_analyzer(errname(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("error type name")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("sentinel error name")),
        "{messages:?}"
    );
}

#[test]
fn errname_allows_valid_names() {
    let dir = support::testdata("errname");
    let pkg = support::typecheck_pkg("example.com/errname/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(errname(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(errname(), &pkg)
    );
}

#[test]
fn errname_ignores_byte_slice_vars() {
    // `[]byte("x")` currently typechecks as Invalid (conversion hole) without
    // a recorded error; implements_error must not treat Invalid/*Invalid as
    // error (Antonboom/errname would never see these as error values).
    let dir = support::testdata("errname");
    let pkg = support::typecheck_pkg("example.com/errname/bytes", &dir.join("bytes_only.go"));
    let messages = support::run_analyzer(errname(), &pkg);
    assert!(
        messages.is_empty(),
        "[]byte var must not be treated as sentinel error: {messages:?}"
    );
}

#[test]
fn err113_flags_direct_error_comparison() {
    let dir = support::testdata("err113");
    let pkg = support::typecheck_pkg("example.com/err113", &dir.join("bad.go"));
    let messages = support::run_analyzer(err113(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("do not compare errors directly")),
        "{messages:?}"
    );
}

#[test]
fn err113_allows_errors_is_style() {
    let dir = support::testdata("err113");
    let pkg = support::typecheck_pkg("example.com/err113/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(err113(), &pkg).is_empty());
}

#[test]
fn durationcheck_flags_duration_times_duration() {
    let dir = support::testdata("durationcheck");
    let pkg = support::typecheck_pkg("example.com/durationcheck", &dir.join("bad.go"));
    let messages = support::run_analyzer(durationcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Multiplication of durations")),
        "{messages:?}"
    );
}

#[test]
fn durationcheck_allows_duration_times_int_cast() {
    let dir = support::testdata("durationcheck");
    let pkg = support::typecheck_pkg("example.com/durationcheck/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(durationcheck(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(durationcheck(), &pkg)
    );
}

#[test]
fn errorlint_flags_error_comparison() {
    let dir = support::testdata("errorlint");
    let pkg = support::typecheck_pkg("example.com/errorlint", &dir.join("bad.go"));
    let messages = support::run_analyzer(errorlint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("comparing with") && m.contains("errors.Is")),
        "{messages:?}"
    );
}

#[test]
fn errorlint_reports_a_parenthesized_nil_comparison() {
    // Found by compat/fuzz.py: the `paren` mutation turned `err != nil` into
    // `err != (nil)` and guff went quiet where golangci-lint did not.
    let dir = support::testdata("errorlint");
    let pkg = support::typecheck_pkg("example.com/errorlint/paren", &dir.join("paren_nil.go"));
    let messages = support::run_analyzer(errorlint(), &pkg);
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("comparing with"))
            .count(),
        1,
        "`err != (nil)` is reported and `err != nil` is not: {messages:?}"
    );
}

#[test]
fn errorlint_allows_nil_check() {
    let dir = support::testdata("errorlint");
    let pkg = support::typecheck_pkg("example.com/errorlint/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(errorlint(), &pkg).is_empty());
}

#[test]
fn wrapcheck_flags_unwrapped_external_error() {
    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck", &dir.join("bad.go"));
    let messages = support::run_analyzer(wrapcheck(), &pkg);
    // The whole message, and the count: `any(contains(..))` passed while the
    // signature's package qualifier was the package *name* rather than its
    // path, which every `ignore-sigs` pattern is also matched against.
    assert_eq!(
        messages,
        vec![WRAPCHECK_JSON_MARSHAL],
        "{messages:?}"
    );
}

/// The whole message for an unwrapped `encoding/json.Marshal`, signature
/// included: the package qualifier is its *path*, which is what go/types
/// prints for a nil qualifier and what `ignore-sigs` is matched against.
const WRAPCHECK_JSON_MARSHAL: &str = "error returned from external package is unwrapped: sig: func encoding/json.Marshal(v any) ([]byte, error)";

#[test]
fn wrapcheck_reports_the_ident_form_from_a_func_literal() {
    // Upstream walks the parent stack looking for a `FuncLit` **inside the
    // call branch only**: `return json.Marshal(v)` from a literal is skipped,
    // but `b, err := json.Marshal(v); return b, err` is reported from one.
    // guff skipped the whole return statement, which is why fiber's
    // `httpReadResponse = func(…) { …; return resp, err }` was silent.
    //
    // The `var b, err = json.Marshal(v)` form has no assignment statement to
    // find at all; upstream falls back to the identifier's own declaration,
    // and guff had no fallback — that shape was silent inside a literal and
    // outside one alike.
    //
    // The message is fixed whole because it carries the signature, and
    // `encoding/json` is the package whose *path* and *name* differ: go/types
    // renders a nil qualifier as the path, so a fixture that only ever calls
    // `os` (where the two are the same) cannot tell the two apart.
    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck/literal", &dir.join("literal.go"));
    let messages = support::run_analyzer(wrapcheck(), &pkg);
    // identInLiteral, varDeclInLiteral, varDeclInDecl, nestedLiterals.
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(
        messages.iter().all(|m| *m == WRAPCHECK_JSON_MARSHAL),
        "{messages:?}"
    );
}

#[test]
fn wrapcheck_allows_wrapped_error() {
    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(wrapcheck(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(wrapcheck(), &pkg)
    );
}

#[test]
fn wrapcheck_ignore_package_globs_skips_encoding() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_error::WrapcheckOptions;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck/globs", &dir.join("bad.go"));
    assert!(
        !support::run_analyzer(wrapcheck(), &pkg).is_empty(),
        "default should flag encoding/json"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "wrapcheck",
        WrapcheckOptions {
            ignore_package_globs: vec!["encoding/*".into()],
            ..WrapcheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        wrapcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "ignore-package-globs encoding/* should silence: {messages:?}"
    );
}

#[test]
fn wrapcheck_extra_ignore_sigs_skips_marshal() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_error::WrapcheckOptions;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck/extra", &dir.join("bad.go"));

    let mut bag = SettingsBag::new();
    bag.insert(
        "wrapcheck",
        WrapcheckOptions {
            extra_ignore_sigs: vec!["encoding/json.Marshal(".into()],
            ..WrapcheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        wrapcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "extra-ignore-sigs should silence Marshal: {messages:?}"
    );
}

#[test]
fn wrapcheck_report_internal_errors() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_error::WrapcheckOptions;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck/internal", &dir.join("internal.go"));
    assert!(
        support::run_analyzer(wrapcheck(), &pkg).is_empty(),
        "default should ignore package-internal: {:?}",
        support::run_analyzer(wrapcheck(), &pkg)
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "wrapcheck",
        WrapcheckOptions {
            report_internal_errors: true,
            ..WrapcheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        wrapcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("package-internal") && m.contains("wrapped")),
        "{messages:?}"
    );
}

#[test]
fn wrapcheck_ignore_interface_regexps() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_error::WrapcheckOptions;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck/iface", &dir.join("iface.go"));
    let default_msgs = support::run_analyzer(wrapcheck(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("interface method") || m.contains("external package")),
        "default should flag interface method: {default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "wrapcheck",
        WrapcheckOptions {
            ignore_interface_regexps: vec!["Reader$".into()],
            ..WrapcheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        wrapcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    // Interface regexp silences the interface-method report; fall-through may
    // still flag as external package unless package-globs also match.
    let mut bag2 = SettingsBag::new();
    bag2.insert(
        "wrapcheck",
        WrapcheckOptions {
            ignore_interface_regexps: vec!["Reader$".into()],
            ignore_package_globs: vec!["example.com/ifacepkg".into()],
            ..WrapcheckOptions::default()
        },
    );
    let silenced = support::run_analyzer_with_settings(
        wrapcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag2),
            ..RunnerOptions::default()
        },
    );
    assert!(
        silenced.is_empty(),
        "interface regexp + package glob should silence: {silenced:?}; intermediate={messages:?}"
    );
}

#[test]
fn errchkjson_flags_blank_and_unsupported() {
    let dir = support::testdata("errchkjson");
    let pkg = support::typecheck_pkg("example.com/errchkjson", &dir.join("bad.go"));
    let messages = support::run_analyzer(errchkjson(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("encoding/json.Marshal") && m.contains("is not checked")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unsupported type") && m.contains("chan")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unsafe type") && m.contains("float64")),
        "{messages:?}"
    );
}

#[test]
fn errchkjson_allows_checked_safe_and_unsafe() {
    let dir = support::testdata("errchkjson");
    let pkg = support::typecheck_pkg("example.com/errchkjson/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(errchkjson(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(errchkjson(), &pkg)
    );
}

#[test]
fn errchkjson_check_error_free_encoding_flags_checked_safe() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_error::ErrchkjsonOptions;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("errchkjson");
    let pkg = support::typecheck_pkg(
        "example.com/errchkjson/cef",
        &dir.join("check_error_free.go"),
    );
    assert!(
        support::run_analyzer(errchkjson(), &pkg).is_empty(),
        "default omit-safe should allow checked safe marshal: {:?}",
        support::run_analyzer(errchkjson(), &pkg)
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "errchkjson",
        ErrchkjsonOptions {
            omit_safe: false,
            report_no_exported: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        errchkjson(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("is checked but passed argument is safe")),
        "{messages:?}"
    );
}

#[test]
fn errchkjson_report_no_exported_flags_empty_struct() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_error::ErrchkjsonOptions;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("errchkjson");
    let pkg = support::typecheck_pkg(
        "example.com/errchkjson/ne",
        &dir.join("no_exported.go"),
    );
    assert!(
        support::run_analyzer(errchkjson(), &pkg).is_empty(),
        "default report-no-exported=false should allow: {:?}",
        support::run_analyzer(errchkjson(), &pkg)
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "errchkjson",
        ErrchkjsonOptions {
            omit_safe: true,
            report_no_exported: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        errchkjson(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("does not contain any exported field")),
        "{messages:?}"
    );
}

#[test]
fn rowserrcheck_flags_missing_err() {
    let dir = support::testdata("rowserrcheck");
    let pkg = support::typecheck_pkg("example.com/rowserrcheck", &dir.join("bad.go"));
    let messages = support::run_analyzer(rowserrcheck(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("rows.Err must be checked")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("rows.Err must be checked"))
            .count()
            >= 2,
        "expected ≥2 diagnostics (missing + reassign): {messages:?}"
    );
}

#[test]
fn rowserrcheck_allows_checked_and_returned() {
    let dir = support::testdata("rowserrcheck");
    let pkg = support::typecheck_pkg("example.com/rowserrcheck/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(rowserrcheck(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn rowserrcheck_packages_setting_enables_sqlx() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_error::RowserrcheckOptions;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("rowserrcheck");
    let pkg = support::typecheck_pkg(
        "example.com/rowserrcheck/settings",
        &dir.join("settings.go"),
    );

    assert!(
        support::run_analyzer(rowserrcheck(), &pkg).is_empty(),
        "default packages (database/sql only) should ignore sqlx: {:?}",
        support::run_analyzer(rowserrcheck(), &pkg)
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "rowserrcheck",
        RowserrcheckOptions {
            packages: vec!["github.com/jmoiron/sqlx".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        rowserrcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("rows.Err must be checked"))
            .count(),
        1,
        "expected one sqlx missing-Err diagnostic: {messages:?}"
    );
}

/// `(*encoding/json.Encoder).Encode` is a **method**, and upstream's rule table
/// keys off `types.Func.FullName()`, which spells methods with a receiver.
/// `code::call_name` cannot produce that — it ends in `func_name`, which is
/// package path plus object name, so the callee came back as
/// `encoding/json.Encode` and the arm never matched. Every Encoder call in the
/// corpus went unreported (7 on syncthing) while the `Marshal` arms, being
/// package functions, kept working and made the linter look healthy.
#[test]
fn errchkjson_flags_unchecked_encoder_encode() {
    let dir = support::testdata("errchkjson");
    let pkg = support::typecheck_pkg("example.com/errchkjson/encoder", &dir.join("encoder.go"));
    let messages = support::run_analyzer(errchkjson(), &pkg);

    let encodes: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("(*encoding/json.Encoder).Encode"))
        .collect();
    assert_eq!(
        encodes.len(),
        4,
        "expected one per unchecked shape (call result, blank assign, variable, \
         unsafe payload) and none for the two checked ones: {messages:?}"
    );
    assert!(
        encodes.iter().any(|m| m.contains("unsafe type")),
        "Encode forces omit-safe, so the float64 payload keeps its suffix: {messages:?}"
    );
}

#[test]
fn errorlint_suggests_errors_as_for_type_assertions() {
    // errorlint's `errors.As` rewrite, `lint.go` ~470-608. Four shapes, and the
    // replacement text is built with errorlint's own `exprToString` walker
    // rather than go/printer.
    //
    // No compat tier can see any of this: the golden key is
    // `path:line:col:linter:severity:text` and carries no suggested-fix body.
    // Verified separately by running `golangci-lint --fix` and `guff run --fix`
    // over the same file and diffing the results, which are byte-identical.
    let dir = support::testdata("errorlint");
    let pkg = support::typecheck_pkg("example.com/errorlint/as", &dir.join("errors_as.go"));
    let fixes: Vec<String> = support::run_analyzer_diagnostics(errorlint(), &pkg)
        .into_iter()
        .filter(|d| d.message.starts_with("type assertion on error"))
        .flat_map(|d| d.suggested_fixes)
        .flat_map(|f| f.text_edits)
        .map(|e| e.new_text)
        .collect();

    for want in [
        // Pointer target composes a value; the declared name is kept.
        "target := &myErr{}\nok := errors.As(err, &target)",
        // The "ok" variable's own name survives.
        "e := &myErr{}\nwasFound := errors.As(err, &e)",
        // `_` is replaced by a name derived from the type, first letter lowered.
        "myErr := &myErr{}\nok := errors.As(err, &myErr)",
        // As an if-initializer, the head of the if is rewritten too.
        "target := &myErr{}\nif errors.As(err, &target)",
        // A non-pointer target is declared, not composed.
        "var target valErr\nok := errors.As(err, &target)",
        // Standalone becomes an immediately-called function literal.
        "func() *myErr {\n\ttarget := &myErr{}\n\t_ = errors.As(err, &target)\n\treturn target\n}()",
    ] {
        assert!(
            fixes.iter().any(|f| f == want),
            "missing fix {want:?} in {fixes:#?}"
        );
    }
}
