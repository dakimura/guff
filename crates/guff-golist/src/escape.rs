//! Module path escaping for GOMODCACHE directory names.
//!
//! Port of `golang.org/x/mod/module.EscapePath`: uppercase runes become
//! `!` + lowercase so the filesystem stays case-insensitive-safe.

/// Escapes a module path for use as a GOMODCACHE directory element.
///
/// Returns `None` if the path already contains `!` (invalid / already escaped).
pub fn escape_path(path: &str) -> Option<String> {
    if path.contains('!') {
        return None;
    }
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        if ch.is_ascii_uppercase() {
            out.push('!');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_uppercase() {
        assert_eq!(
            escape_path("github.com/Azure/go-autorest").as_deref(),
            Some("github.com/!azure/go-autorest")
        );
    }

    #[test]
    fn leaves_lowercase_alone() {
        assert_eq!(
            escape_path("golang.org/x/sys").as_deref(),
            Some("golang.org/x/sys")
        );
    }

    #[test]
    fn rejects_already_escaped() {
        assert!(escape_path("github.com/!azure/go-autorest").is_none());
    }
}
