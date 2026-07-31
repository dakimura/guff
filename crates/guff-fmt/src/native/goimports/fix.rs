//! Import fix pass — port of `golang.org/x/tools/internal/imports` getFixes
//! (delete unused + sibling/stdlib add). Module-cache resolution omitted.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use guff::ast::{ast_is_exported, Decl, Expr, File, Spec, ValueSpec};
use guff::format::FormatError as AstFormatError;
use guff::parser::{Mode as ParserMode, ALL_ERRORS, PARSE_COMMENTS, SKIP_OBJECT_RESOLUTION, SKIP_STAMP_NODE_IDS};
use guff::parser_interface;
use guff::walk::{self, NodeRef};
use guff::FileSet;

const PARSER_MODE_SIBLING: ParserMode = ParserMode(
    PARSE_COMMENTS.0 | SKIP_OBJECT_RESOLUTION.0 | SKIP_STAMP_NODE_IDS.0 | ALL_ERRORS.0,
);

const C_IMPORT: &str = "\"C\"";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    pub name: String,
    pub import_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportFixType {
    AddImport = 0,
    DeleteImport = 1,
    SetImportName = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportFix {
    pub stmt: ImportInfo,
    pub ident_name: String,
    pub fix_type: ImportFixType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatOutcome {
    Formatted(Vec<u8>),
    NeedsResolver,
}

pub(crate) enum FixesResult {
    Fixes(Vec<ImportFix>),
    NeedsResolver,
}

type References = BTreeMap<String, BTreeSet<String>>;

struct PackageInfo {
    name: String,
    exports: HashSet<String>,
}

struct Pass<'a> {
    f: &'a File,
    other_files: Vec<File>,
    load_real_package_names: bool,
    last_try: bool,

    existing_imports: HashMap<String, Vec<ImportInfo>>,
    all_refs: References,
    missing_refs: References,
    candidates: Vec<ImportInfo>,
    known_packages: HashMap<String, PackageInfo>,
}

/// Compute import fixes for `file`. Mirrors `getFixesWithSource` without
/// external/module-cache candidates.
pub(crate) fn get_fixes(
    fset: &Arc<FileSet>,
    file: &File,
    filename: &str,
    _src: &[u8],
) -> Result<FixesResult, AstFormatError> {
    let abs = std::fs::canonicalize(filename)
        .unwrap_or_else(|_| PathBuf::from(filename));
    let src_dir = abs
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    // Pass 1: self only, assumed package names.
    let mut p = Pass {
        f: file,
        other_files: Vec::new(),
        load_real_package_names: false,
        last_try: false,
        existing_imports: HashMap::new(),
        all_refs: References::new(),
        missing_refs: References::new(),
        candidates: Vec::new(),
        known_packages: HashMap::new(),
    };
    if let Some(fixes) = p.load()? {
        return Ok(FixesResult::Fixes(fixes));
    }

    // Pass 2: siblings.
    p.other_files = parse_other_files(fset, &src_dir, filename)?;
    if let Some(fixes) = p.load()? {
        return Ok(FixesResult::Fixes(fixes));
    }

    // Stdlib + sibling-derived candidates.
    p.assume_sibling_imports_valid();
    add_stdlib_candidates(&mut p);
    if let Some(fixes) = p.fix() {
        return Ok(FixesResult::Fixes(fixes));
    }

    // No module resolver: give up for unresolved third-party refs.
    Ok(FixesResult::NeedsResolver)
}

pub(crate) fn collect_imports(f: &File) -> Vec<ImportInfo> {
    let mut imports = Vec::new();
    for imp in &f.imports {
        let name = imp
            .name
            .as_ref()
            .map(|n| n.name.clone())
            .unwrap_or_default();
        if imp.path.value == C_IMPORT || name == "_" || name == "." {
            continue;
        }
        let path = trim_quotes(&imp.path.value);
        imports.push(ImportInfo {
            name,
            import_path: path,
        });
    }
    imports
}

fn trim_quotes(v: &str) -> String {
    v.trim_matches('"').to_string()
}

fn collect_references(f: &File) -> References {
    let mut refs = References::new();
    walk::inspect(NodeRef::File(f), |n| {
        let Some(NodeRef::SelectorExpr(sel)) = n else {
            return true;
        };
        let Expr::Ident(xident) = sel.x.as_ref() else {
            return true;
        };
        if xident.obj.lock().unwrap().is_some() {
            return true;
        }
        if !ast_is_exported(&sel.sel.name) {
            return true;
        }
        refs.entry(xident.name.clone())
            .or_default()
            .insert(sel.sel.name.clone());
        true
    });
    refs
}

fn add_globals(f: &File, globals: &mut HashSet<String>) {
    for decl in &f.decls {
        let Decl::GenDecl(gen) = decl else {
            continue;
        };
        for spec in &gen.specs {
            let Spec::ValueSpec(ValueSpec { names, .. }) = spec else {
                continue;
            };
            // x/tools only records Names[0].
            if let Some(n) = names.first() {
                globals.insert(n.name.clone());
            }
        }
    }
}

fn parse_other_files(
    fset: &Arc<FileSet>,
    src_dir: &Path,
    filename: &str,
) -> Result<Vec<File>, AstFormatError> {
    let consider_tests = filename.ends_with("_test.go");
    let file_base = Path::new(filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let entries = match std::fs::read_dir(src_dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    let mut files = Vec::new();
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.as_ref() == file_base || !name.ends_with(".go") {
            continue;
        }
        if !consider_tests && name.ends_with("_test.go") {
            continue;
        }
        let path = ent.path();
        let Ok(src) = std::fs::read(&path) else {
            continue;
        };
        let path_str = path.to_string_lossy();
        let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parser_interface::parse_file(fset, &path_str, Some(&src), PARSER_MODE_SIBLING)
        }));
        match parsed {
            Ok(Ok(f)) => files.push(f),
            _ => continue,
        }
    }
    Ok(files)
}

impl Pass<'_> {
    fn load(&mut self) -> Result<Option<Vec<ImportFix>>, AstFormatError> {
        self.known_packages.clear();
        self.missing_refs.clear();
        self.existing_imports.clear();

        self.all_refs = collect_references(self.f);

        let mut globals = HashSet::new();
        self.candidates.clear();
        let pkg_name = self.f.name.name.clone();
        for other in &self.other_files {
            if other.name.name == pkg_name {
                add_globals(other, &mut globals);
            }
            self.candidates.extend(collect_imports(other));
        }

        let imports = collect_imports(self.f);
        // load_real_package_names left false → assumed names only.
        let _ = self.load_real_package_names;

        for imp in &imports {
            let ident = self.import_identifier(imp);
            self.existing_imports
                .entry(ident)
                .or_default()
                .push(imp.clone());
        }

        for (left, rights) in &self.all_refs {
            if globals.contains(left) {
                continue;
            }
            if !self.existing_imports.contains_key(left) {
                self.missing_refs.insert(left.clone(), rights.clone());
            }
        }
        if !self.missing_refs.is_empty() {
            return Ok(None);
        }
        Ok(self.fix())
    }

    fn fix(&mut self) -> Option<Vec<ImportFix>> {
        let mut selected = Vec::new();
        for (left, rights) in &self.missing_refs {
            if let Some(imp) = self.find_missing_import(left, rights) {
                selected.push(imp);
            }
        }

        if !self.last_try && selected.len() != self.missing_refs.len() {
            return None;
        }

        let mut fixes = Vec::new();
        for imps in self.existing_imports.values() {
            for imp in imps {
                let ident = self.import_identifier(imp);
                if !self.all_refs.contains_key(&ident) {
                    fixes.push(ImportFix {
                        stmt: imp.clone(),
                        ident_name: ident,
                        fix_type: ImportFixType::DeleteImport,
                    });
                    continue;
                }
                // SetImportName skipped without loadRealPackageNames.
            }
        }
        sort_fixes(&mut fixes);

        let mut selected_fixes = Vec::new();
        for imp in &selected {
            let name = self.import_spec_name(imp);
            selected_fixes.push(ImportFix {
                stmt: ImportInfo {
                    name,
                    import_path: imp.import_path.clone(),
                },
                ident_name: self.import_identifier(imp),
                fix_type: ImportFixType::AddImport,
            });
        }
        sort_fixes(&mut selected_fixes);
        fixes.extend(selected_fixes);
        Some(fixes)
    }

    fn find_missing_import(&self, pkg: &str, syms: &BTreeSet<String>) -> Option<ImportInfo> {
        for candidate in &self.candidates {
            let Some(pkg_info) = self.known_packages.get(&candidate.import_path) else {
                continue;
            };
            if self.import_identifier(candidate) != pkg {
                continue;
            }
            if syms.iter().all(|s| pkg_info.exports.contains(s)) {
                return Some(candidate.clone());
            }
        }
        None
    }

    fn import_identifier(&self, imp: &ImportInfo) -> String {
        if !imp.name.is_empty() {
            return imp.name.clone();
        }
        if let Some(known) = self.known_packages.get(&imp.import_path) {
            if !known.name.is_empty() {
                return without_version(&known.name);
            }
        }
        import_path_to_assumed_name(&imp.import_path)
    }

    fn import_spec_name(&self, imp: &ImportInfo) -> String {
        if self.load_real_package_names && imp.name.is_empty() {
            let ident = self.import_identifier(imp);
            if ident == import_path_to_assumed_name(&imp.import_path) {
                return String::new();
            }
            return ident;
        }
        imp.name.clone()
    }

    fn assume_sibling_imports_valid(&mut self) {
        let others: Vec<File> = self.other_files.clone();
        for f in &others {
            let refs = collect_references(f);
            let imports = collect_imports(f);
            let mut by_name: HashMap<String, ImportInfo> = HashMap::new();
            for imp in &imports {
                by_name.insert(self.import_identifier(imp), imp.clone());
            }
            for (left, rights) in refs {
                if let Some(imp) = by_name.get(&left) {
                    let exports: HashSet<String> =
                        if let Some(std) = stdlib_exports().get(imp.import_path.as_str()) {
                            std.clone()
                        } else {
                            rights.into_iter().collect()
                        };
                    self.add_candidate(
                        imp.clone(),
                        PackageInfo {
                            name: String::new(),
                            exports,
                        },
                    );
                }
            }
        }
    }

    fn add_candidate(&mut self, imp: ImportInfo, pkg: PackageInfo) {
        self.candidates.push(imp.clone());
        if let Some(existing) = self.known_packages.get_mut(&imp.import_path) {
            if existing.name.is_empty() {
                existing.name = pkg.name;
            }
            existing.exports.extend(pkg.exports);
        } else {
            self.known_packages.insert(imp.import_path, pkg);
        }
    }
}

fn sort_fixes(fixes: &mut [ImportFix]) {
    fixes.sort_by(|fi, fj| {
        (&fi.stmt.import_path, &fi.stmt.name, &fi.ident_name, fi.fix_type).cmp(&(
            &fj.stmt.import_path,
            &fj.stmt.name,
            &fj.ident_name,
            fj.fix_type,
        ))
    });
}

fn add_stdlib_candidates(p: &mut Pass<'_>) {
    let refs: Vec<String> = p.missing_refs.keys().cloned().collect();
    for left in refs {
        if left == "rand" {
            add_stdlib_pkg(p, "crypto/rand");
            add_stdlib_pkg(p, "math/rand/v2");
            add_stdlib_pkg(p, "math/rand");
            continue;
        }
        for import_path in stdlib_exports().keys() {
            if path_base(import_path) == left {
                add_stdlib_pkg(p, import_path);
            }
        }
    }
}

fn add_stdlib_pkg(p: &mut Pass<'_>, pkg: &str) {
    // Prevent self-imports (best-effort; GOROOT check omitted).
    if path_base(pkg) == p.f.name.name {
        return;
    }
    let exports = stdlib_exports()
        .get(pkg)
        .cloned()
        .unwrap_or_default();
    let name = local_base(pkg);
    p.add_candidate(
        ImportInfo {
            name: String::new(),
            import_path: pkg.to_string(),
        },
        PackageInfo { name, exports },
    );
}

fn local_base(nm: &str) -> String {
    let mut ans = path_base(nm).to_string();
    if ans.starts_with('v') && ans[1..].parse::<u32>().is_ok() {
        if let Some(ix) = nm.rfind(&ans) {
            if ix > 0 {
                let parent = &nm[..ix.saturating_sub(1)];
                let more = path_base(parent);
                ans = format!("{more}/{ans}");
            }
        }
    }
    ans
}

fn path_base(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn without_version(nm: &str) -> String {
    let v = path_base(nm);
    if v.starts_with('v') && v[1..].parse::<u32>().is_ok() && v.len() < nm.len() {
        let xnm = &nm[..nm.len() - v.len() - 1];
        return path_base(xnm).to_string();
    }
    nm.to_string()
}

/// Port of `ImportPathToAssumedName`.
pub(crate) fn import_path_to_assumed_name(import_path: &str) -> String {
    let mut base = path_base(import_path).to_string();
    if base.starts_with('v') && base[1..].parse::<u32>().is_ok() {
        let dir = {
            let i = import_path.rfind('/').unwrap_or(0);
            if i == 0 {
                "."
            } else {
                &import_path[..i]
            }
        };
        if dir != "." {
            base = path_base(dir).to_string();
        }
    }
    if let Some(rest) = base.strip_prefix("go-") {
        base = rest.to_string();
    }
    if let Some(i) = base.char_indices().find(|&(_, c)| !is_ident_cont(c)).map(|(i, _)| i) {
        // Keep prefix that is a valid identifier start+cont.
        let prefix: String = base
            .chars()
            .take_while(|c| is_ident_cont(*c))
            .collect();
        // Prefer cutting at first non-identifier as Go does via IndexFunc.
        let _ = i;
        if !prefix.is_empty() {
            // Go: base = base[:i] where i is first non-identifier.
            let cut: String = base
                .chars()
                .enumerate()
                .take_while(|(idx, c)| {
                    if *idx == 0 {
                        is_ident_start(*c)
                    } else {
                        is_ident_cont(*c)
                    }
                })
                .map(|(_, c)| c)
                .collect();
            return cut;
        }
    }
    // Trim to identifier prefix.
    base.chars()
        .enumerate()
        .take_while(|(i, c)| {
            if *i == 0 {
                is_ident_start(*c)
            } else {
                is_ident_cont(*c)
            }
        })
        .map(|(_, c)| c)
        .collect()
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_cont(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn stdlib_exports() -> &'static HashMap<String, HashSet<String>> {
    static EXPORTS: OnceLock<HashMap<String, HashSet<String>>> = OnceLock::new();
    EXPORTS.get_or_init(|| {
        let mut m = HashMap::new();
        for line in include_str!("stdlib_exports.txt").lines() {
            if line.is_empty() {
                continue;
            }
            let Some((path, names)) = line.split_once('\t') else {
                continue;
            };
            let set: HashSet<String> = names.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
            m.insert(path.to_string(), set);
        }
        m
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assumed_name_strips_go_prefix() {
        assert_eq!(import_path_to_assumed_name("github.com/foo/go-bar"), "bar");
        assert_eq!(import_path_to_assumed_name("math/rand/v2"), "rand");
    }

    #[test]
    fn stdlib_has_fmt_println() {
        let e = stdlib_exports().get("fmt").expect("fmt");
        assert!(e.contains("Println"));
    }
}
