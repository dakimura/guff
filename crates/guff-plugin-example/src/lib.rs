//! Example module plugin for guff.
//!
//! Mirrors [golangci/example-plugin-module-linter](https://github.com/golangci/example-plugin-module-linter):
//! reports `// TODO:` comments that lack an author.
//!
//! Link this crate into a binary built by `guff custom`. Registration happens
//! via [`guff_plugin::register!`] (inventory), equivalent to Go `init()` +
//! `register.Plugin`.

use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff_plugin::guff_analysis::passes::inspect;
use guff_plugin::{
    decode_settings, AnalysisResult, Analyzer, LinterPlugin, Pass, PluginError, RunError, RunFn,
};
use serde::Deserialize;
use serde_yaml::Value;

guff_plugin::register!("example", new_example);

/// Force the linker to keep this crate when depended on from a custom binary.
pub const FORCE_LINK: () = ();

#[derive(Debug, Clone, Default, Deserialize)]
struct MySettings {
    /// Decoded from `linters.settings.custom.example.settings` (demo field).
    #[serde(default)]
    one: String,
}

struct PluginExample {
    settings: MySettings,
}

fn new_example(settings: &Value) -> Result<Box<dyn LinterPlugin>, PluginError> {
    let s = decode_settings::<MySettings>(settings)?;
    let _ = SETTINGS.set(s.clone());
    Ok(Box::new(PluginExample { settings: s }))
}

static SETTINGS: OnceLock<MySettings> = OnceLock::new();

impl LinterPlugin for PluginExample {
    fn build_analyzers(&self) -> Result<Vec<&'static Analyzer>, PluginError> {
        let _ = &self.settings;
        Ok(vec![analyzer()])
    }

    fn description(&self) -> &'static str {
        "find TODOs without an author"
    }
}

fn reparse_with_comments(path: &Path) -> Option<(Arc<FileSet>, guff::ast::File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "example requires inspect analyzer".to_string())?;

    let from_bag = pass
        .settings::<Value>("example")
        .cloned()
        .unwrap_or(Value::Null);
    let opts = decode_settings::<MySettings>(&from_bag)
        .ok()
        .or_else(|| SETTINGS.get().cloned())
        .unwrap_or_default();
    let _ = opts.one.as_str();

    let mut pending = Vec::new();
    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let fset = pass.fset().clone();
    let n = pass.files().len();

    for i in 0..n {
        let file = &pass.files()[i];
        let Some(path) = paths.get(i) else {
            continue;
        };
        let Some((re_fset, parsed)) = reparse_with_comments(path) else {
            continue;
        };
        for cg in &parsed.comments {
            for c in &cg.list {
                let mut found = Vec::new();
                check_todo_comment(&c.text, c.slash.0 as u32, &mut found);
                if found.is_empty() {
                    continue;
                }
                // Map line from reparsed file onto the Pass FileSet.
                let line = re_fset.position(c.slash).line;
                let pos = fset
                    .file(file.pos())
                    .and_then(|ft| {
                        if line < 1 || line as usize > ft.line_count() {
                            None
                        } else {
                            Some(ft.line_start(line as usize).0 as u32)
                        }
                    })
                    .unwrap_or(c.slash.0 as u32);
                for (_, msg) in found {
                    pending.push((pos, msg));
                }
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn check_todo_comment(text: &str, pos: u32, pending: &mut Vec<(u32, String)>) {
    for prefix in ["// TODO:", "// TODO():"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let rest = rest.trim_start();
            // Author form: "// TODO(alice): fix me"
            if rest.is_empty() || !rest.starts_with('(') {
                pending.push((pos, "TODO comment has no author".into()));
            }
            return;
        }
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "example",
        doc: "find TODOs without an author",
        url: "https://github.com/golangci/example-plugin-module-linter",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_settings_roundtrip() {
        let yaml: Value = serde_yaml::from_str("one: yes\n").unwrap();
        let s = decode_settings::<MySettings>(&yaml).unwrap();
        assert_eq!(s.one, "yes");
    }

    #[test]
    fn factory_builds_analyzer() {
        guff_plugin::clear_instances();
        let analyzers = guff_plugin::instantiate("example", &Value::Null).unwrap();
        assert_eq!(analyzers.len(), 1);
        assert_eq!(analyzers[0].name, "example");
        guff_plugin::clear_instances();
    }

    #[test]
    fn detects_todo_without_author() {
        let mut pending = Vec::new();
        check_todo_comment("// TODO: fix", 1, &mut pending);
        assert_eq!(pending.len(), 1);
        pending.clear();
        check_todo_comment("// TODO(alice): fix", 1, &mut pending);
        assert!(pending.is_empty());
    }
}
