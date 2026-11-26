//! CamelCase / initialism naming (revive `var-naming`).

use std::collections::HashSet;

const COMMON_INITIALISMS: &[&str] = &[
    "ACL", "API", "ASCII", "CPU", "CSS", "DNS", "EOF", "GUID", "HTML", "HTTP", "HTTPS", "ID",
    "IDS", "IP", "JSON", "LHS", "QPS", "RAM", "RHS", "RPC", "SLA", "SMTP", "SQL", "SSH", "TCP",
    "TLS", "TTL", "UDP", "UI", "UID", "UUID", "URI", "URL", "UTF8", "VM", "XML", "XMPP", "XSRF",
    "XSS",
];

/// Returns the canonical name for `name` (golint/revive `rule.Name`).
pub fn canonical_name(name: &str) -> String {
    canonical_name_with_lists(name, &[], &[], false)
}

pub fn canonical_name_with_lists(
    name: &str,
    allowlist: &[&str],
    blocklist: &[&str],
    skip_initialism_checks: bool,
) -> String {
    if name == "_" {
        return name.into();
    }
    if name.chars().all(|c| c.is_ascii_lowercase()) {
        return name.into();
    }

    let mut runes: Vec<char> = name.chars().collect();
    let mut allow: HashSet<&str> = allowlist.iter().copied().collect();
    let mut block: HashSet<&str> = blocklist.iter().copied().collect();
    let _ = (&mut allow, &mut block);

    let mut w = 0usize;
    let mut i = 0usize;
    while i + 1 <= runes.len() {
        let mut eow = i + 1 == runes.len();
        if !eow {
            if runes[i + 1] == '_' {
                eow = true;
                let mut n = 1usize;
                while i + n + 1 < runes.len() && runes[i + n + 1] == '_' {
                    n += 1;
                }
                if i + n + 1 < runes.len()
                    && runes[i].is_ascii_digit()
                    && runes[i + n + 1].is_ascii_digit()
                    && n > 0
                {
                    n -= 1;
                }
                runes.drain(i + 1..i + 1 + n);
            } else if runes[i].is_ascii_lowercase() && !runes[i + 1].is_ascii_lowercase() {
                eow = true;
            }
        }
        i += 1;
        if !eow {
            continue;
        }

        let word: String = runes[w..i].iter().collect();
        let upper = word.to_ascii_uppercase();
        let is_init = COMMON_INITIALISMS.contains(&upper.as_str()) || block.contains(upper.as_str());
        let ignore = allow.contains(upper.as_str());
        if !skip_initialism_checks && is_init && !ignore {
            let mut u = upper;
            if w == 0 && runes[w].is_ascii_lowercase() {
                u = u.to_ascii_lowercase();
            }
            if u == "IDS" {
                u = "IDs".into();
            }
            let uchars: Vec<char> = u.chars().collect();
            runes.splice(w..w + word.chars().count(), uchars);
            i = w + u.chars().count();
        } else if w > 0 && word.chars().all(|c| c.is_ascii_lowercase()) {
            runes[w] = runes[w].to_ascii_uppercase();
        }
        w = i;
    }
    runes.into_iter().collect()
}

pub fn is_upper_underscore(s: &str) -> bool {
    if !s.contains('_') || s.len() <= 5 {
        return false;
    }
    s.chars()
        .all(|c| c == '_' || c.is_ascii_uppercase() || c.is_ascii_digit())
}
