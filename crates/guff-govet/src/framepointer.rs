//! `framepointer` — check assembly that clobbers the frame pointer before saving it.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/framepointer`.

use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use guff::position::File;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

struct Arch {
    is_fp_write: fn(&str) -> bool,
    is_fp_read: fn(&str) -> bool,
    is_unconditional_branch: fn(&str) -> bool,
}

fn has_any_prefix(s: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| s.starts_with(p))
}

fn contains_word(s: &str, word: &str) -> bool {
    let bytes = s.as_bytes();
    let wlen = word.len();
    if wlen == 0 || s.len() < wlen {
        return false;
    }
    let mut start = 0;
    while let Some(pos) = s[start..].find(word) {
        let at = start + pos;
        let end = at + wlen;
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn amd64_is_fp_write(line: &str) -> bool {
    let line = line.trim();
    if let Some(i) = line.rfind(',') {
        return line[i + 1..].trim() == "BP";
    }
    false
}

fn amd64_is_fp_read(line: &str) -> bool {
    contains_word(line, "BP")
}

fn amd64_is_unconditional_branch(line: &str) -> bool {
    has_any_prefix(line.trim(), &["JMP", "RET"])
}

fn arm64_is_fp_write(s: &str) -> bool {
    let s = s.trim();
    if let Some(i) = s.rfind(',') {
        if i > 0 && s[i..].ends_with("R29") {
            return true;
        }
    }
    if has_any_prefix(s, &["LDP", "LDAXP", "LDXP", "CASP"]) {
        let lp = s.rfind('(');
        let rp = s.rfind(')');
        if let (Some(lp), Some(rp)) = (lp, rp) {
            if lp < rp {
                let inner = &s[lp..rp];
                return inner.contains(',') && inner.contains("R29");
            }
        }
    }
    false
}

fn arm64_is_fp_read(line: &str) -> bool {
    contains_word(line, "R29")
}

fn arm64_is_unconditional_branch(line: &str) -> bool {
    let instr = line.trim().split_whitespace().next().unwrap_or(line);
    instr == "B" || instr == "JMP" || instr == "RET"
}

fn goarch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("amd64"),
        "aarch64" => Some("arm64"),
        _ => None,
    }
}

fn goos() -> Option<&'static str> {
    match std::env::consts::OS {
        "linux" => Some("linux"),
        "macos" => Some("darwin"),
        _ => None,
    }
}

fn arch_for_host() -> Option<Arch> {
    match goarch()? {
        "amd64" => Some(Arch {
            is_fp_write: amd64_is_fp_write,
            is_fp_read: amd64_is_fp_read,
            is_unconditional_branch: amd64_is_unconditional_branch,
        }),
        "arm64" => Some(Arch {
            is_fp_write: arm64_is_fp_write,
            is_fp_read: arm64_is_fp_read,
            is_unconditional_branch: arm64_is_unconditional_branch,
        }),
        _ => None,
    }
}

fn read_asm_file(pass: &Pass<'_>, path: &str) -> Result<(Arc<File>, String), RunError> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("framepointer: read {path}: {e}"))?;
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    let fset = pass.fset();
    for f in fset.files() {
        if f.name() == name {
            return Ok((f, content));
        }
    }
    let file = fset.add_file(name, -1, content.len() as i64);
    file.set_lines_for_content(content.as_bytes());
    Ok((file, content))
}

fn check_asm_file(pass: &mut Pass<'_>, arch: &Arch, path: &str) -> Result<(), RunError> {
    let (file, content) = read_asm_file(pass, path)?;
    let mut active = false;
    for (lineno, line) in content.split_inclusive('\n').enumerate() {
        let lineno = lineno + 1;
        let line = if let Some(i) = line.find("//") {
            &line[..i]
        } else {
            line
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("TEXT") && line.contains("(SB)") && line.contains("$0") {
            active = true;
            continue;
        }
        if !active {
            continue;
        }

        if (arch.is_fp_write)(line) {
            pass.reportf(file.line_start(lineno).0 as u32, "frame pointer is clobbered before saving");
            active = false;
            continue;
        }
        if (arch.is_fp_read)(line) || (arch.is_unconditional_branch)(line) {
            active = false;
        }
    }
    Ok(())
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if goos().is_none() {
        return Ok(None);
    }
    let Some(arch) = arch_for_host() else {
        return Ok(None);
    };
    if pass.pkg().pkg_path == "runtime" {
        return Ok(None);
    }

    let asm_files: Vec<String> = pass
        .other_files()
        .iter()
        .filter(|path| path.ends_with(".s"))
        .cloned()
        .collect();
    for path in asm_files {
        check_asm_file(pass, &arch, &path)?;
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "framepointer",
        doc: "report assembly that clobbers the frame pointer before saving it",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/framepointer",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amd64_detects_bp_write() {
        assert!(amd64_is_fp_write("MOVQ $0, BP"));
        assert!(amd64_is_fp_write("MOVQ AX, BP"));
        assert!(!amd64_is_fp_write("MOVQ BP, BX"));
    }

    #[test]
    fn arm64_detects_r29_write() {
        assert!(arm64_is_fp_write("MOVD $0, R29"));
        assert!(arm64_is_fp_write("LDP 0(R1), (R26, R29)"));
        assert!(!arm64_is_fp_write("MOVD R29, R1"));
    }
}
