//! Minimal ANSI color helpers matching fatih/color sequences used by golangci-lint tests.
//!
//! Bold = `\x1b[1m…\x1b[22m`, FgRed = `\x1b[31m…\x1b[0m`, FgYellow = `\x1b[33m…\x1b[0m`.

/// Wrap `s` in bold when `enabled`.
pub(crate) fn bold(enabled: bool, s: &str) -> String {
    if enabled {
        format!("\x1b[1m{s}\x1b[22m")
    } else {
        s.to_string()
    }
}

/// Wrap `s` in red foreground when `enabled`.
pub(crate) fn red(enabled: bool, s: &str) -> String {
    if enabled {
        format!("\x1b[31m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// Wrap `s` in yellow foreground when `enabled`.
pub(crate) fn yellow(enabled: bool, s: &str) -> String {
    if enabled {
        format!("\x1b[33m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}
