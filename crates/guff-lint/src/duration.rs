//! Go-style duration parsing (`5m`, `1h30m`, `30s`, …) for `--timeout` / `run.timeout`.

use std::time::Duration;

/// Parse a Go `time.ParseDuration`–compatible string.
///
/// Accepts unit suffixes `ns`, `us`/`µs`, `ms`, `s`, `m`, `h` (optionally stacked,
/// e.g. `1h30m`). A bare `0` is [`Duration::ZERO`].
pub fn parse_go_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".into());
    }
    if s == "0" {
        return Ok(Duration::ZERO);
    }

    let bytes = s.as_bytes();
    let mut i = 0;
    let mut total = Duration::ZERO;
    let mut saw_unit = false;

    while i < bytes.len() {
        // Optional leading sign (only `+` / no `-` for timeouts).
        if bytes[i] == b'+' {
            i += 1;
        }
        if i >= bytes.len() || !bytes[i].is_ascii_digit() {
            return Err(format!("invalid duration {s:?}"));
        }

        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        // Fractional part.
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        let num: f64 = std::str::from_utf8(&bytes[start..i])
            .ok()
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| format!("invalid duration number in {s:?}"))?;

        if i >= bytes.len() {
            return Err(format!("missing unit in duration {s:?}"));
        }

        let unit_start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        // Also accept µ (UTF-8 C2 B5) / μ (UTF-8 CE BC) as micro prefix.
        if unit_start == i {
            // Try multi-byte micro sign.
            if s[unit_start..].starts_with('µ') || s[unit_start..].starts_with('μ') {
                i = unit_start + s[unit_start..].chars().next().unwrap().len_utf8();
                if i < bytes.len() && bytes[i] == b's' {
                    i += 1;
                }
            } else {
                return Err(format!("missing unit in duration {s:?}"));
            }
        }
        let unit = &s[unit_start..i];
        let unit_secs: f64 = match unit {
            "ns" => 1e-9,
            "us" | "µs" | "μs" => 1e-6,
            "ms" => 1e-3,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            _ => return Err(format!("unknown duration unit {unit:?} in {s:?}")),
        };
        saw_unit = true;
        let nanos = (num * unit_secs * 1e9).round() as u128;
        total = total.saturating_add(Duration::from_nanos(
            u64::try_from(nanos.min(u128::from(u64::MAX))).unwrap_or(u64::MAX),
        ));
    }

    if !saw_unit {
        return Err(format!("invalid duration {s:?}"));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_timeouts() {
        assert_eq!(parse_go_duration("0").unwrap(), Duration::ZERO);
        assert_eq!(parse_go_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_go_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(
            parse_go_duration("1h30m").unwrap(),
            Duration::from_secs(5400)
        );
        assert_eq!(parse_go_duration("1.5s").unwrap(), Duration::from_millis(1500));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_go_duration("").is_err());
        assert!(parse_go_duration("5").is_err());
        assert!(parse_go_duration("5x").is_err());
    }
}
