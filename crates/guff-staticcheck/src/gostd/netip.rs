//! Port of `net/netip.ParseAddr`, error paths only.
//!
//! `net/url.parseHost` calls it on the contents of an IP-literal (`[…]`) and
//! wraps whatever comes back as `invalid host: <err>`, so SA1007's message for
//! a malformed IPv6 host is `netip`'s message verbatim. The parsed address
//! itself is discarded except for one bit — whether the input was dotted-quad
//! IPv4, which `parseHost` rejects — so [`parse_addr`] returns only that.
//!
//! The input is bytes rather than `&str`: it has already been through
//! `unescape`, which lets `%FF` through in a host and so can produce a string
//! that is not valid UTF-8.

use super::strconv::quote_bytes;

/// Mirrors `netip.parseAddrError`.
struct AddrError<'a> {
    input: &'a [u8],
    msg: &'static str,
    at: Option<&'a [u8]>,
}

impl AddrError<'_> {
    fn to_message(&self) -> String {
        match self.at {
            Some(at) => format!(
                "ParseAddr({}): {} (at {})",
                quote_bytes(self.input),
                self.msg,
                quote_bytes(at)
            ),
            None => format!("ParseAddr({}): {}", quote_bytes(self.input), self.msg),
        }
    }
}

type AddrResult<'a, T> = Result<T, AddrError<'a>>;

/// `netip.ParseAddr`, reduced to "did it parse, and was it an IPv4 address?".
///
/// `Ok(true)` means the input was dotted-quad IPv4 — the case `parseHost`
/// rejects with `invalid IP-literal`, since only IPv6 may be bracketed. An
/// IPv4-mapped IPv6 address such as `::ffff:1.2.3.4` reaches `AddrFrom16` and
/// so reports `Ok(false)`, which is what lets it through.
pub fn parse_addr(s: &[u8]) -> Result<bool, String> {
    let result = match s.iter().find(|c| matches!(c, b'.' | b':' | b'%')) {
        Some(b'.') => parse_ipv4(s).map(|()| true),
        Some(b':') => parse_ipv6(s).map(|()| false),
        Some(b'%') => Err(AddrError {
            input: s,
            msg: "missing IPv6 address",
            at: None,
        }),
        _ => Err(AddrError {
            input: s,
            msg: "unable to parse IP",
            at: None,
        }),
    };
    result.map_err(|e| e.to_message())
}

/// Mirrors `netip.parseIPv4Fields`. Only the error matters, so the four octets
/// are parsed and dropped.
fn parse_ipv4_fields(input: &[u8], off: usize, end: usize) -> AddrResult<'_, ()> {
    let err = |msg, at| Err(AddrError { input, msg, at });
    let s = &input[off..end];
    let mut val: u32 = 0;
    let mut pos = 0;
    let mut dig_len = 0;
    for i in 0..s.len() {
        if s[i].is_ascii_digit() {
            if dig_len == 1 && val == 0 {
                return err("IPv4 field has octet with leading zero", None);
            }
            val = val * 10 + u32::from(s[i] - b'0');
            dig_len += 1;
            if val > 255 {
                return err("IPv4 field has value >255", None);
            }
        } else if s[i] == b'.' {
            // ".1.2.3", "1.2.3.", "1..2.3"
            if i == 0 || i == s.len() - 1 || s[i - 1] == b'.' {
                return err("IPv4 field must have at least one digit", Some(&s[i..]));
            }
            // "1.2.3.4.5"
            if pos == 3 {
                return err("IPv4 address too long", None);
            }
            pos += 1;
            val = 0;
            dig_len = 0;
        } else {
            return err("unexpected character", Some(&s[i..]));
        }
    }
    if pos < 3 {
        return err("IPv4 address too short", None);
    }
    Ok(())
}

fn parse_ipv4(s: &[u8]) -> AddrResult<'_, ()> {
    parse_ipv4_fields(s, 0, s.len())
}

/// Mirrors `netip.parseIPv6`.
fn parse_ipv6(input: &[u8]) -> AddrResult<'_, ()> {
    let err = |msg, at| Err(AddrError { input, msg, at });
    let mut s = input;
    let mut zone: &[u8] = b"";

    // The zone is split off up front, as upstream does.
    if let Some(i) = s.iter().position(|&c| c == b'%') {
        (s, zone) = (&s[..i], &s[i + 1..]);
        if zone.is_empty() {
            return err("zone must be a non-empty string", None);
        }
    }

    let mut ellipsis: isize = -1;

    // Might have a leading ellipsis.
    if s.len() >= 2 && s[0] == b':' && s[1] == b':' {
        ellipsis = 0;
        s = &s[2..];
        if s.is_empty() {
            return Ok(()); // "::" alone
        }
    }

    let mut i: usize = 0;
    while i < 16 {
        // Hex group.
        let mut off = 0;
        let mut acc: u32 = 0;
        while off < s.len() {
            let c = s[off];
            let digit = match c {
                b'0'..=b'9' => u32::from(c - b'0'),
                b'a'..=b'f' => u32::from(c - b'a') + 10,
                b'A'..=b'F' => u32::from(c - b'A') + 10,
                _ => break,
            };
            acc = (acc << 4) + digit;
            if off > 3 {
                return err("each group must have 4 or less digits", Some(s));
            }
            if acc > u32::from(u16::MAX) {
                return err("IPv6 field has value >=2^16", Some(s));
            }
            off += 1;
        }
        if off == 0 {
            return err(
                "each colon-separated field must have at least one digit",
                Some(s),
            );
        }

        // A dot here means a trailing embedded IPv4 address.
        if off < s.len() && s[off] == b'.' {
            if ellipsis < 0 && i != 12 {
                return err(
                    "embedded IPv4 address must replace the final 2 fields of the address",
                    Some(s),
                );
            }
            if i + 4 > 16 {
                return err(
                    "too many hex fields to fit an embedded IPv4 at the end of the address",
                    Some(s),
                );
            }
            let mut end = input.len();
            if !zone.is_empty() {
                end -= zone.len() + 1;
            }
            parse_ipv4_fields(input, end - s.len(), end)?;
            s = b"";
            i += 4;
            break;
        }

        i += 2;

        s = &s[off..];
        if s.is_empty() {
            break;
        }

        // Otherwise a colon, and more after it.
        if s[0] != b':' {
            return err("unexpected character, want colon", Some(s));
        } else if s.len() == 1 {
            return err("colon must be followed by more characters", Some(s));
        }
        s = &s[1..];

        if s[0] == b':' {
            if ellipsis >= 0 {
                return err("multiple :: in address", Some(s));
            }
            ellipsis = i as isize;
            s = &s[1..];
            if s.is_empty() {
                break;
            }
        }
    }

    if !s.is_empty() {
        return err("trailing garbage after address", Some(s));
    }
    if i < 16 {
        if ellipsis < 0 {
            return err("address string too short", None);
        }
    } else if ellipsis >= 0 {
        return err("the :: must expand to at least one field of zeros", None);
    }
    Ok(())
}
