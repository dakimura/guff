mod support;

use std::sync::Arc;

use guff_analysis::validate;
use guff_analysis::SettingsBag;
use guff_dupl::{dupl, Options};
use guff_runner::{run_on_packages, RunnerOptions};

/// The duplicate's location is a **path**, not a basename.
///
/// Upstream renders it with `fsutils.ShortestRelPath(i.To.Filename(), "")` —
/// relative to the working directory. The difference is invisible until a
/// config excludes by `text`: gitea drops dupl findings matching `(?i)webhook`,
/// which its `services/webhook/*.go` duplicates carry only because the path is
/// in the message. Eight findings golangci-lint does not report.
#[test]
fn dupl_flags_duplicate_functions() {
    let pkg = support::typecheck_fixture("dupl", "example.com/dupl", "bad.go");
    let messages = run_dupl_with_threshold(&pkg, 30);
    assert!(
        messages.iter().any(|m| {
            m.contains("3-34 lines are duplicate of `tests/testdata/dupl/bad.go:36-67`")
                || m.contains("36-67 lines are duplicate of `tests/testdata/dupl/bad.go:3-34`")
        }),
        "the message names the path relative to the working directory \
         (`cargo test` runs in the crate dir): {messages:?}"
    );
}

#[test]
fn dupl_allows_distinct_code() {
    let pkg = support::typecheck_fixture("dupl", "example.com/dupl/ok", "ok.go");
    assert!(run_dupl_with_threshold(&pkg, 30).is_empty());
}

#[test]
fn dupl_analyzer_graph_is_valid() {
    validate(&[dupl()]).expect("valid analyzer graph");
}

fn run_dupl_with_threshold(pkg: &Arc<guff_packages::Package>, threshold: i32) -> Vec<String> {
    let mut bag = SettingsBag::new();
    bag.insert("dupl", Options { threshold });
    let result = run_on_packages(
        &[dupl()],
        std::slice::from_ref(pkg),
        &RunnerOptions {
            sequential: true,
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    )
    .expect("run analyzer");
    for action in result.graph.all_actions() {
        if let Some(err) = action.error() {
            panic!("analyzer {} failed: {err}", action.string_id());
        }
    }
    result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| d.message)
        .collect()
}
