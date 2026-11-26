mod support;

use std::sync::Arc;

use guff_analysis::SettingsBag;
use guff_import::{
    analyzer_block_logrus, analyzer_local_replace, depguard, gomoddirectives, gomodguard, importas,
    DenyEntry, DepguardOptions, DepguardRule, GomoddirectivesOptions, GomodguardOptions,
    ImportasAlias, ImportasOptions, ListMode,
};
use guff_runner::RunnerOptions;

#[test]
fn depguard_flags_non_stdlib_imports() {
    let pkg = support::typecheck_fixture("depguard", "example.com/depguard", "bad.go");
    let messages = support::run_analyzer(depguard(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("github.com/foo/bar") && m.contains("not allowed")),
        "{messages:?}"
    );
}

#[test]
fn depguard_allows_stdlib() {
    let pkg = support::typecheck_fixture("depguard", "example.com/depguard/ok", "ok.go");
    assert!(support::run_analyzer(depguard(), &pkg).is_empty());
}

#[test]
fn depguard_lax_deny_via_settings() {
    let pkg = support::typecheck_fixture(
        "depguard",
        "example.com/depguard/lax",
        "lax_deny.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "depguard",
        DepguardOptions {
            rules: vec![DepguardRule {
                name: "Main".into(),
                list_mode: ListMode::Lax,
                files: vec!["$all".into(), "!$test".into()],
                allow: Vec::new(),
                deny: vec![DenyEntry {
                    pkg: "github.com/sirupsen/logrus".into(),
                    desc: "use log/slog".into(),
                }],
            }],
        },
    );
    let messages = support::run_analyzer_with_settings(
        depguard(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("logrus") && m.contains("use log/slog")),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("fmt")),
        "fmt should be allowed under lax deny-only: {messages:?}"
    );
}

#[test]
fn gomoddirectives_flags_replace() {
    let pkg = support::typecheck_fixture(
        "gomoddirectives/bad",
        "example.com/gomoddirectives/bad",
        "main.go",
    );
    let messages = support::run_analyzer(gomoddirectives(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("replacement") || m.contains("local replacement")),
        "{messages:?}"
    );
}

#[test]
fn gomoddirectives_allows_clean_gomod() {
    let pkg = support::typecheck_fixture(
        "gomoddirectives/ok",
        "example.com/gomoddirectives/ok",
        "main.go",
    );
    assert!(support::run_analyzer(gomoddirectives(), &pkg).is_empty());
}

#[test]
fn gomoddirectives_replace_local_via_settings() {
    let pkg = support::typecheck_fixture(
        "gomoddirectives/replacelocal",
        "example.com/gomoddirectives/replacelocal",
        "main.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "gomoddirectives",
        GomoddirectivesOptions {
            replace_local: true,
            ..GomoddirectivesOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gomoddirectives(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "local replace should be allowed: {messages:?}"
    );
}

#[test]
fn gomoddirectives_exclude_forbidden_via_settings() {
    let pkg = support::typecheck_fixture(
        "gomoddirectives/exclude",
        "example.com/gomoddirectives/exclude",
        "main.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "gomoddirectives",
        GomoddirectivesOptions {
            exclude_forbidden: true,
            ..GomoddirectivesOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gomoddirectives(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("exclude")),
        "{messages:?}"
    );
}

#[test]
fn gomodguard_default_is_quiet() {
    let pkg = support::typecheck_fixture("gomodguard/ok", "example.com/gomodguard/ok", "main.go");
    assert!(support::run_analyzer(gomodguard(), &pkg).is_empty());
}

#[test]
fn gomodguard_flags_blocked_module_import() {
    let pkg = support::typecheck_fixture(
        "gomodguard/blocked",
        "example.com/gomodguard/blocked",
        "main.go",
    );
    let messages = support::run_analyzer(analyzer_block_logrus(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("logrus") && m.contains("blocked")),
        "{messages:?}"
    );
}

/// The message `BlockedModule.BlockReason` builds, and the second `Sprintf`
/// that runs over it.
///
/// gomodguard assembles `"%s %s"` from a constant carrying the `%s` for the
/// package name and the per-module reason, and then — in
/// `isBlockedPackageFromModFile` — runs `fmt.Sprintf(blockReason, packageName)`
/// over the *whole* thing. So any verb the user wrote in `reason` meets Go's
/// formatter with no argument left: beats blocks `github.com/pkg/errors` with
/// "use `fmt.Errorf` with `%w` instead" and golangci-lint prints
/// `%!w(MISSING)`.
///
/// guff had none of this: no recommendation clause, no second `Sprintf`, no
/// `TrimRight(reason, ".")`, and an empty reason rendered as a bare `.`. All
/// five shapes below were measured against golangci-lint 2.12.2 and all five
/// differed.
#[test]
fn gomodguard_renders_every_block_reason_shape() {
    let pkg = support::typecheck_fixture(
        "gomodguard/messages",
        "example.com/gomodguard/messages",
        "main.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "gomodguard",
        GomodguardOptions {
            blocked_modules: vec![
                guff_import::BlockedModule {
                    module: "example.com/bare".into(),
                    recommendations: Vec::new(),
                    reason: String::new(),
                },
                guff_import::BlockedModule {
                    module: "example.com/onerec".into(),
                    recommendations: vec!["errors".into()],
                    reason: "This package is deprecated, use `errors.Join` instead".into(),
                },
                guff_import::BlockedModule {
                    module: "example.com/tworecs".into(),
                    recommendations: vec!["errors".into(), "fmt".into()],
                    reason: "This package is deprecated, use `fmt.Errorf` with `%w` instead"
                        .into(),
                },
                guff_import::BlockedModule {
                    module: "example.com/threerecs".into(),
                    recommendations: vec!["a".into(), "b".into(), "c".into()],
                    reason: "Three recommendations, and a trailing dot.".into(),
                },
                guff_import::BlockedModule {
                    module: "example.com/verbs".into(),
                    recommendations: Vec::new(),
                    reason: "No recommendations at all, with a %s and a %%literal".into(),
                },
            ],
            local_replace_directives: false,
        },
    );
    let mut messages = support::run_analyzer_with_settings(
        gomodguard(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    messages.sort();

    const LIST: &str = "is blocked because the module is in the blocked modules list.";
    assert_eq!(
        messages,
        vec![
            // No recommendations and no reason: upstream still joins with a
            // space, and the trailing space is what the text printer trims.
            format!("import of package `example.com/bare` {LIST} "),
            // One recommendation reads "is a recommended module".
            format!(
                "import of package `example.com/onerec` {LIST} `errors` is a recommended \
                 module. This package is deprecated, use `errors.Join` instead."
            ),
            // Three: a comma after the first, a bare space after the second.
            format!(
                "import of package `example.com/threerecs` {LIST} `a`, `b` and `c` are \
                 recommended modules. Three recommendations, and a trailing dot."
            ),
            // Two: no comma at all, and the reason's `%w` has no argument left.
            format!(
                "import of package `example.com/tworecs` {LIST} `errors` and `fmt` are \
                 recommended modules. This package is deprecated, use `fmt.Errorf` with \
                 `%!w(MISSING)` instead."
            ),
            // `%s` is the second verb, so it is MISSING too; `%%` is a literal.
            format!(
                "import of package `example.com/verbs` {LIST} No recommendations at all, \
                 with a %!s(MISSING) and a %literal."
            ),
        ],
        "{messages:?}"
    );
}

#[test]
fn gomodguard_flags_blocked_via_settings() {
    let pkg = support::typecheck_fixture(
        "gomodguard/blocked",
        "example.com/gomodguard/blocked/settings",
        "main.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "gomodguard",
        GomodguardOptions {
            blocked_modules: vec![guff_import::BlockedModule {
                module: "github.com/sirupsen/logrus".into(),
                recommendations: vec!["log/slog".into()],
                reason: "use log/slog".into(),
            }],
            local_replace_directives: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        gomodguard(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("logrus") && m.contains("blocked")),
        "{messages:?}"
    );
}

#[test]
fn gomodguard_flags_local_replace_import() {
    let pkg = support::typecheck_fixture(
        "gomodguard/localreplace",
        "example.com/gomodguard/localreplace",
        "main.go",
    );
    let messages = support::run_analyzer(analyzer_local_replace(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("local replace") && m.contains("github.com/foo/bar")),
        "{messages:?}"
    );
}

fn importas_alias_bag() -> SettingsBag {
    let mut bag = SettingsBag::new();
    bag.insert(
        "importas",
        ImportasOptions {
            alias: vec![
                ImportasAlias {
                    pkg: "fmt".into(),
                    alias: "fmtpkg".into(),
                },
                ImportasAlias {
                    pkg: "os".into(),
                    alias: "ospkg".into(),
                },
            ],
            no_unaliased: false,
            no_extra_aliases: false,
        },
    );
    bag
}

#[test]
fn importas_flags_wrong_aliases() {
    let pkg = support::typecheck_fixture("importas", "example.com/importas/bad", "bad.go");
    let messages = support::run_analyzer_with_settings(
        importas(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(importas_alias_bag()),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("fmt") && m.contains("fmtpkg")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os") && m.contains("ospkg")),
        "{messages:?}"
    );
}

/// Renaming the alias without renaming its uses writes code that does not
/// compile.
///
/// `import f "fmt"` becomes `import fmtpkg "fmt"`, so every `f.Sprintf` has to
/// become `fmtpkg.Sprintf` in the same fix. guff used to emit the import line
/// alone, and `undefined: f` is invisible to every finding-set gate — the
/// diagnostic is identical either way (COMPAT-HARDENING, `compat/fix/`).
#[test]
fn importas_fix_renames_the_use_sites_too() {
    let pkg = support::typecheck_fixture("importas", "example.com/importas/bad", "bad.go");
    let diags = support::run_analyzer_diagnostics(
        importas(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(importas_alias_bag()),
            ..RunnerOptions::default()
        },
    );
    let mut checked = 0;
    for d in &diags {
        let (want_import, want_use) = if d.message.contains("\"fmt\"") {
            ("fmtpkg \"fmt\"", "fmtpkg")
        } else if d.message.contains("\"os\"") {
            ("ospkg \"os\"", "ospkg")
        } else {
            continue;
        };
        let edits = &d.suggested_fixes[0].text_edits;
        assert_eq!(
            edits[0].new_text, want_import,
            "the import line comes first: {edits:?}"
        );
        assert_eq!(
            edits.len(),
            2,
            "one import line plus its single use site: {edits:?}"
        );
        assert_eq!(edits[1].new_text, want_use, "{edits:?}");
        // A rename, not a deletion: the span is the old qualifier alone.
        assert!(edits[1].end > edits[1].pos, "{edits:?}");
        checked += 1;
    }
    assert_eq!(checked, 2, "both imports are misaliased: {diags:?}");
}

#[test]
fn importas_allows_correct_aliases() {
    let pkg = support::typecheck_fixture("importas", "example.com/importas/ok", "ok.go");
    let messages = support::run_analyzer_with_settings(
        importas(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(importas_alias_bag()),
            ..RunnerOptions::default()
        },
    );
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn importas_no_unaliased_flags_missing_alias() {
    let pkg = support::typecheck_fixture(
        "importas",
        "example.com/importas/nounaliased",
        "no_unaliased.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "importas",
        ImportasOptions {
            alias: vec![ImportasAlias {
                pkg: "fmt".into(),
                alias: "fmtpkg".into(),
            }],
            no_unaliased: true,
            no_extra_aliases: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        importas(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("without alias") && m.contains("fmtpkg")),
        "{messages:?}"
    );
}

#[test]
fn importas_no_extra_aliases_flags_unknown() {
    let pkg = support::typecheck_fixture("importas", "example.com/importas/extra", "extra.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "importas",
        ImportasOptions {
            alias: Vec::new(),
            no_unaliased: false,
            no_extra_aliases: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        importas(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("not part of config") && m.contains("fmt")),
        "{messages:?}"
    );
}

#[test]
fn importas_regex_capture_alias() {
    let pkg = support::typecheck_fixture("importas", "example.com/importas/regex", "regex.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "importas",
        ImportasOptions {
            alias: vec![ImportasAlias {
                pkg: r"net/(\w+)".into(),
                alias: "$1pkg".into(),
            }],
            no_unaliased: false,
            no_extra_aliases: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        importas(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("httppkg") && m.contains("net/http")),
        "{messages:?}"
    );
}
