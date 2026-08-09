//! Port of `net/url.Parse`, error paths only (`src/net/url/url.go`).
//!
//! SA1007 calls `url.Parse` on a constant and reports `err.Error()`, so the
//! error text *is* the check. The two things a Rust URL crate cannot supply
//! are exactly the two things that matter: `url::Url` implements the WHATWG
//! spec, not RFC 3986 as Go reads it (it rejects `foobar` and `mailto:a@b.c`,
//! which Go accepts), and its messages share no wording with Go's.
//!
//! Everything downstream of the last error return is dropped — the parsed
//! `URL` struct is never inspected — so this reports only whether Go would have
//! failed, and how it would have said so.

use super::netip;
use super::strconv::{quote, quote_bytes};

/// `url.Parse(raw_url)`, reduced to its error.
///
/// `Ok(())` means Go would have returned a `*URL`. The `Err` is
/// `(*url.Error).Error()`, i.e. `parse <quoted url>: <cause>`.
pub fn parse(raw_url: &str) -> Result<(), String> {
    let b = raw_url.as_bytes();
    // Cut off #frag.
    let (u, frag) = match b.iter().position(|&c| c == b'#') {
        Some(i) => (&b[..i], &b[i + 1..]),
        None => (b, &b[b.len()..]),
    };
    if let Err(cause) = parse_url(u) {
        return Err(format!("parse {}: {}", quote_bytes(u), cause));
    }
    if frag.is_empty() {
        return Ok(());
    }
    // setFragment
    if let Err(cause) = unescape(frag, Encoding::Fragment) {
        return Err(format!("parse {}: {}", quote(raw_url), cause));
    }
    Ok(())
}

/// Mirrors `url.parse(rawURL, viaRequest=false)`.
///
/// `ParseRequestURI`'s `viaRequest=true` arms are omitted: SA1007's rule table
/// names only `net/url.Parse`.
fn parse_url(raw_url: &[u8]) -> Result<(), String> {
    if raw_url.iter().any(|&b| b < b' ' || b == 0x7f) {
        return Err("net/url: invalid control character in URL".into());
    }
    if raw_url == b"*" {
        return Ok(());
    }

    let (scheme, mut rest) = get_scheme(raw_url)?;
    let scheme = scheme.to_ascii_lowercase();

    if rest.last() == Some(&b'?') && rest.iter().filter(|&&c| c == b'?').count() == 1 {
        // ForceQuery
        rest = &rest[..rest.len() - 1];
    } else if let Some(i) = rest.iter().position(|&c| c == b'?') {
        // The query is not validated here; ParseQuery is a separate call.
        rest = &rest[..i];
    }

    if !rest.starts_with(b"/") {
        if !scheme.is_empty() {
            // A rootless path is opaque per RFC 3986; nothing left to check.
            return Ok(());
        }
        // Avoid confusion with malformed schemes like `cache_object:foo/bar`:
        // RFC 3986 §3.3 forbids a colon in the first segment of a relative
        // path reference.
        let segment = match rest.iter().position(|&c| c == b'/') {
            Some(i) => &rest[..i],
            None => rest,
        };
        if segment.contains(&b':') {
            return Err("first path segment in URL cannot contain colon".into());
        }
    }

    if (!scheme.is_empty() || !rest.starts_with(b"///")) && rest.starts_with(b"//") {
        let mut authority = &rest[2..];
        let mut tail: &[u8] = b"";
        if let Some(i) = authority.iter().position(|&c| c == b'/') {
            (authority, tail) = (&authority[..i], &authority[i..]);
        }
        parse_authority(&scheme, authority)?;
        rest = tail;
    }

    // setPath
    unescape(rest, Encoding::Path)?;
    Ok(())
}

/// Mirrors `url.getScheme`: `scheme:path`, where scheme is
/// `[a-zA-Z][a-zA-Z0-9+.-]*`.
fn get_scheme(raw_url: &[u8]) -> Result<(&[u8], &[u8]), String> {
    for (i, &c) in raw_url.iter().enumerate() {
        match c {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if i == 0 {
                    return Ok((b"", raw_url));
                }
            }
            b':' => {
                if i == 0 {
                    return Err("missing protocol scheme".into());
                }
                return Ok((&raw_url[..i], &raw_url[i + 1..]));
            }
            // An invalid character means there is no scheme at all.
            _ => return Ok((b"", raw_url)),
        }
    }
    Ok((b"", raw_url))
}

/// Mirrors `url.parseAuthority`.
fn parse_authority(scheme: &[u8], authority: &[u8]) -> Result<(), String> {
    let at = authority.iter().rposition(|&c| c == b'@');
    match at {
        None => parse_host(scheme, authority)?,
        Some(i) => parse_host(scheme, &authority[i + 1..])?,
    }
    let Some(i) = at else {
        return Ok(());
    };
    let userinfo = &authority[..i];
    if !valid_userinfo(userinfo) {
        return Err("net/url: invalid userinfo".into());
    }
    match userinfo.iter().position(|&c| c == b':') {
        None => {
            unescape(userinfo, Encoding::UserPassword)?;
        }
        Some(i) => {
            unescape(&userinfo[..i], Encoding::UserPassword)?;
            unescape(&userinfo[i + 1..], Encoding::UserPassword)?;
        }
    }
    Ok(())
}

/// Mirrors `url.parseHost`.
fn parse_host(scheme: &[u8], host: &[u8]) -> Result<(), String> {
    match host.iter().rposition(|&c| c == b'[') {
        Some(open) if open > 0 => return Err("invalid IP-literal".into()),
        Some(_) => {
            // An IP-Literal per RFC 3986 / RFC 6874: "[fe80::1]",
            // "[fe80::1%25en0]", "[fe80::1]:80".
            let Some(close) = host.iter().rposition(|&c| c == b']') else {
                return Err("missing ']' in host".into());
            };
            let colon_port = &host[close + 1..];
            if !valid_optional_port(colon_port) {
                return Err(format!("invalid port {} after host", quote_bytes(colon_port)));
            }
            unescape(colon_port, Encoding::Host)?;

            let hostname = &host[1..close];
            // RFC 6874: %25 introduces the zone identifier, which may use
            // almost any escaping; the host itself may only escape non-ASCII.
            let unescaped = match index_of(hostname, b"%25") {
                Some(zone_idx) => {
                    let mut host_part = unescape(&hostname[..zone_idx], Encoding::Host)?;
                    let zone_part = unescape(&hostname[zone_idx..], Encoding::Zone)?;
                    host_part.extend_from_slice(&zone_part);
                    host_part
                }
                None => unescape(hostname, Encoding::Host)?,
            };

            // Only a valid IPv6 address may be bracketed. IPv4-mapped
            // addresses are not excluded — only dotted-quad IPv4.
            match netip::parse_addr(&unescaped) {
                Err(e) => return Err(format!("invalid host: {e}")),
                Ok(true) => return Err("invalid IP-literal".into()),
                Ok(false) => return Ok(()),
            }
        }
        None => {
            // RFC 3986 does not allow a colon in the host subcomponent, but
            // PostgreSQL and MongoDB URLs carry comma-separated host:port
            // lists, so Go takes the *last* colon as the port separator.
            //
            // Go 1.26 made http/https take the first instead, behind the
            // `urlstrictcolons` godebug, whose default follows the main
            // module's go directive (go.dev/issue/75223) — which is why
            // upstream's signature takes `scheme` and this one ignores it.
            // golangci-lint v2.12.2 declares `go 1.25.0`, so the pre-1.26
            // behaviour is what guff has to reproduce; `compat/oracles/gourl`
            // pins the same directive and gates the choice.
            let _ = scheme;
            if let Some(i) = host.iter().rposition(|&c| c == b':') {
                let colon_port = &host[i..];
                if !valid_optional_port(colon_port) {
                    return Err(format!("invalid port {} after host", quote_bytes(colon_port)));
                }
            }
        }
    }
    unescape(host, Encoding::Host)?;
    Ok(())
}

/// Mirrors `url.validOptionalPort`: empty, or `:` followed by digits only.
fn valid_optional_port(port: &[u8]) -> bool {
    if port.is_empty() {
        return true;
    }
    port[0] == b':' && port[1..].iter().all(u8::is_ascii_digit)
}

/// Mirrors `url.validUserinfo`.
///
/// Go iterates runes, so any non-ASCII rune falls into the default arm and
/// makes the userinfo invalid; iterating bytes reaches the same verdict.
fn valid_userinfo(s: &[u8]) -> bool {
    s.iter().all(|&c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                b'-' | b'.'
                    | b'_'
                    | b':'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'%'
                    // RFC 3986 §3.2.1 forbids it, but real URLs such as
                    // "http://username:p@ssword@google.com" carry one.
                    | b'@'
            )
    })
}

fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// Mirrors `url.encoding`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Path,
    PathSegment,
    Host,
    Zone,
    UserPassword,
    QueryComponent,
    Fragment,
}

/// Mirrors `url.shouldEscape` — the reference implementation in
/// `gen_encoding_table.go`, from which Go generates its lookup table.
fn should_escape(c: u8, mode: Encoding) -> bool {
    // §2.3 unreserved characters (alphanum)
    if c.is_ascii_alphanumeric() {
        return false;
    }

    if mode == Encoding::Host || mode == Encoding::Zone {
        // §3.2.2 allows sub-delims in reg-name. Go adds `:` because it keeps
        // `:port` in Host, `[` `]` for `[ipv6]:port`, and `<` `>` `"` because
        // they are the only characters left that could possibly be allowed
        // (a host cannot %-encode ASCII, so escaping them would reject them).
        if matches!(
            c,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'['
                | b']'
                | b'<'
                | b'>'
                | b'"'
        ) {
            return false;
        }
    }

    match c {
        // §2.3 unreserved characters (mark)
        b'-' | b'_' | b'.' | b'~' => return false,
        // §2.2 reserved characters: a few may appear unescaped, per section.
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => {
            match mode {
                // §3.3. The RFC allows : @ & = + $ but reserves / ; , for
                // giving meaning to individual segments; this package handles
                // the path as a whole, so only ? has to be escaped.
                Encoding::Path => return c == b'?',
                Encoding::PathSegment => {
                    return c == b'/' || c == b';' || c == b',' || c == b'?'
                }
                // §3.2.1. The RFC allows ; : & = + $ , in userinfo, so only
                // @ / ? must be escaped — and : too, since parsing treats it
                // as the username/password separator.
                Encoding::UserPassword => {
                    return c == b'@' || c == b'/' || c == b'?' || c == b':'
                }
                // §3.4. The RFC reserves everything.
                Encoding::QueryComponent => return true,
                // §4.1. The text is silent but the grammar allows everything.
                Encoding::Fragment => return false,
                Encoding::Host | Encoding::Zone => {}
            }
        }
        _ => {}
    }

    if mode == Encoding::Fragment {
        // RFC 3986 §2.2 allows sub-delims unescaped. Go escapes them anywhere
        // but the fragment, and always escapes the single quote, to avoid
        // breaking callers that relied on it (go.dev/issue/19917).
        if matches!(c, b'!' | b'(' | b')' | b'*') {
            return false;
        }
    }

    true
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c) || (b'A'..=b'F').contains(&c)
}

/// Precondition: `is_hex(c)`.
fn unhex(c: u8) -> u8 {
    9 * (c >> 6) + (c & 15)
}

fn escape_error(s: &[u8]) -> String {
    format!("invalid URL escape {}", quote_bytes(s))
}

fn invalid_host_error(s: &[u8]) -> String {
    format!("invalid character {} in host name", quote_bytes(s))
}

/// Mirrors `url.unescape`. Returns the unescaped bytes, or Go's error text.
fn unescape(s: &[u8], mode: Encoding) -> Result<Vec<u8>, String> {
    // Count %, check that they are well-formed.
    let mut n = 0;
    let mut has_plus = false;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                n += 1;
                if i + 2 >= s.len() || !is_hex(s[i + 1]) || !is_hex(s[i + 2]) {
                    let bad = &s[i..];
                    return Err(escape_error(&bad[..bad.len().min(3)]));
                }
                // RFC 3986 p.21: in the host component, %-encoding is only for
                // non-ASCII bytes — except %25, which RFC 6874 §2 allows so an
                // IPv6 scoped-address literal can escape its percent sign.
                if mode == Encoding::Host && unhex(s[i + 1]) < 8 && &s[i..i + 3] != b"%25" {
                    return Err(escape_error(&s[i..i + 3]));
                }
                if mode == Encoding::Zone {
                    // RFC 6874 says anything goes in a zone identifier, but Go
                    // restricts %-escaped bytes to those that would be legal
                    // unescaped — plus the space, which Windows puts there.
                    let v = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                    if &s[i..i + 3] != b"%25" && v != b' ' && should_escape(v, Encoding::Host) {
                        return Err(escape_error(&s[i..i + 3]));
                    }
                }
                i += 3;
            }
            b'+' => {
                has_plus = mode == Encoding::QueryComponent;
                i += 1;
            }
            c => {
                if (mode == Encoding::Host || mode == Encoding::Zone)
                    && c < 0x80
                    && should_escape(c, mode)
                {
                    return Err(invalid_host_error(&s[i..i + 1]));
                }
                i += 1;
            }
        }
    }

    if n == 0 && !has_plus {
        return Ok(s.to_vec());
    }

    let unescaped_plus = if mode == Encoding::QueryComponent {
        b' '
    } else {
        b'+'
    };
    let mut out = Vec::with_capacity(s.len() - 2 * n);
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                out.push((unhex(s[i + 1]) << 4) | unhex(s[i + 2]));
                i += 3;
            }
            b'+' => {
                out.push(unescaped_plus);
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}
