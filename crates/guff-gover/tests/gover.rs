//! Port of `internal/gover/gover_test.go`.

use guff_gover::{compare, is_lang, is_valid, lang, parse, Version};

fn v(major: &str, minor: &str, patch: &str, kind: &str, pre: &str) -> Version {
    Version {
        major: major.into(),
        minor: minor.into(),
        patch: patch.into(),
        kind: kind.into(),
        pre: pre.into(),
    }
}

#[test]
fn compare_table() {
    let cases: &[(&str, &str, i32)] = &[
        ("", "", 0),
        ("x", "x", 0),
        ("", "x", 0),
        ("1", "1.1", -1),
        ("1.5", "1.6", -1),
        ("1.5", "1.10", -1),
        ("1.6", "1.6.1", -1),
        ("1.19", "1.19.0", 0),
        ("1.19rc1", "1.19", -1),
        ("1.20", "1.20.0", 0),
        ("1.20rc1", "1.20", -1),
        ("1.21", "1.21.0", -1),
        ("1.21", "1.21rc1", -1),
        ("1.21rc1", "1.21.0", -1),
        ("1.6", "1.19", -1),
        ("1.19", "1.19.1", -1),
        ("1.19rc1", "1.19", -1),
        ("1.19rc1", "1.19.1", -1),
        ("1.19rc1", "1.19rc2", -1),
        ("1.19.0", "1.19.1", -1),
        ("1.19rc1", "1.19.0", -1),
        ("1.19alpha3", "1.19beta2", -1),
        ("1.19beta2", "1.19rc1", -1),
        ("1.1", "1.99999999999999998", -1),
        ("1.99999999999999998", "1.99999999999999999", -1),
    ];
    for (x, y, want) in cases {
        assert_eq!(compare(x, y), *want, "compare({:?}, {:?})", x, y);
    }
}

#[test]
fn parse_table() {
    let cases: &[(&str, Version)] = &[
        ("1", v("1", "0", "0", "", "")),
        ("1.2", v("1", "2", "0", "", "")),
        ("1.2.3", v("1", "2", "3", "", "")),
        ("1.2rc3", v("1", "2", "", "rc", "3")),
        ("1.20", v("1", "20", "0", "", "")),
        ("1.21", v("1", "21", "", "", "")),
        ("1.21rc3", v("1", "21", "", "rc", "3")),
        ("1.21.0", v("1", "21", "0", "", "")),
        ("1.24", v("1", "24", "", "", "")),
        ("1.24rc3", v("1", "24", "", "rc", "3")),
        ("1.24.0", v("1", "24", "0", "", "")),
        ("1.999testmod", v("1", "999", "", "testmod", "")),
        ("1.99999999999999999", v("1", "99999999999999999", "", "", "")),
    ];
    for (input, want) in cases {
        assert_eq!(parse(input), *want, "parse({:?})", input);
    }
}

#[test]
fn lang_table() {
    let cases: &[(&str, &str)] = &[
        ("1.2rc3", "1.2"),
        ("1.2.3", "1.2"),
        ("1.2", "1.2"),
        ("1", "1"),
        ("1.999testmod", "1.999"),
    ];
    for (input, want) in cases {
        assert_eq!(lang(input), *want, "lang({:?})", input);
    }
}

#[test]
fn is_lang_table() {
    let cases: &[(&str, bool)] = &[
        ("1.2rc3", false),
        ("1.2.3", false),
        ("1.999testmod", false),
        ("1.22", true),
        ("1.21", true),
        ("1.20", false),
        ("1.19", false),
        ("1.3", false),
        ("1.2", false),
        ("1", false),
    ];
    for (input, want) in cases {
        assert_eq!(is_lang(input), *want, "is_lang({:?})", input);
    }
}

#[test]
fn is_valid_table() {
    let cases: &[(&str, bool)] = &[
        ("1.2rc3", true),
        ("1.2.3", true),
        ("1.999testmod", true),
        ("1.600+auto", false),
        ("1.22", true),
        ("1.21.0", true),
        ("1.21rc2", true),
        ("1.21", true),
        ("1.20.0", true),
        ("1.20", true),
        ("1.19", true),
        ("1.3", true),
        ("1.2", true),
        ("1", true),
    ];
    for (input, want) in cases {
        assert_eq!(is_valid(input), *want, "is_valid({:?})", input);
    }
}

#[test]
fn dec_int_basics() {
    use guff_gover::dec_int;
    assert_eq!(dec_int("1"), "0");
    assert_eq!(dec_int("10"), "9");
    assert_eq!(dec_int("100"), "99");
    assert_eq!(dec_int("1000"), "999");
    assert_eq!(dec_int("21"), "20");
    assert_eq!(dec_int("0"), "");
    assert_eq!(dec_int("00"), "");
}

#[test]
fn max_basics() {
    use guff_gover::max;
    assert_eq!(max("1.20", "1.21"), "1.21");
    assert_eq!(max("1.21", "1.20"), "1.21");
    // Equal returns x (the first arg).
    assert_eq!(max("1.21", "1.21"), "1.21");
}
