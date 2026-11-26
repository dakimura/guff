//! Port of `go/version/version_test.go`.

use guff_version::{compare, is_valid, lang};

#[test]
fn compare_table() {
    let cases: &[(&str, &str, i32)] = &[
        ("", "", 0),
        ("x", "x", 0),
        ("", "x", 0),
        ("1", "1.1", 0),
        ("go1", "go1.1", -1),
        ("go1.5", "go1.6", -1),
        ("go1.5", "go1.10", -1),
        ("go1.6", "go1.6.1", -1),
        ("go1.19", "go1.19.0", 0),
        ("go1.19rc1", "go1.19", -1),
        ("go1.20", "go1.20.0", 0),
        ("go1.20", "go1.20.0-bigcorp", 0),
        ("go1.20rc1", "go1.20", -1),
        ("go1.21", "go1.21.0", -1),
        ("go1.21", "go1.21.0-bigcorp", -1),
        ("go1.21", "go1.21rc1", -1),
        ("go1.21rc1", "go1.21.0", -1),
        ("go1.6", "go1.19", -1),
        ("go1.19", "go1.19.1", -1),
        ("go1.19rc1", "go1.19.1", -1),
        ("go1.19rc1", "go1.19rc2", -1),
        ("go1.19.0", "go1.19.1", -1),
        ("go1.19rc1", "go1.19.0", -1),
        ("go1.19alpha3", "go1.19beta2", -1),
        ("go1.19beta2", "go1.19rc1", -1),
        ("go1.1", "go1.99999999999999998", -1),
        ("go1.99999999999999998", "go1.99999999999999999", -1),
    ];
    for (x, y, want) in cases {
        assert_eq!(compare(x, y), *want, "compare({:?}, {:?})", x, y);
    }
}

#[test]
fn lang_table() {
    let cases: &[(&str, &str)] = &[
        ("bad", ""),
        ("go1.2rc3", "go1.2"),
        ("go1.2.3", "go1.2"),
        ("go1.2", "go1.2"),
        ("go1", "go1"),
        ("go222", "go222.0"),
        ("go1.999testmod", "go1.999"),
    ];
    for (input, want) in cases {
        assert_eq!(lang(input), *want, "lang({:?})", input);
    }
}

#[test]
fn is_valid_table() {
    let cases: &[(&str, bool)] = &[
        ("", false),
        ("1.2.3", false),
        ("go1.2rc3", true),
        ("go1.2.3", true),
        ("go1.999testmod", true),
        ("go1.600+auto", false),
        ("go1.22", true),
        ("go1.21.0", true),
        ("go1.21rc2", true),
        ("go1.21", true),
        ("go1.20.0", true),
        ("go1.20", true),
        ("go1.19", true),
        ("go1.3", true),
        ("go1.2", true),
        ("go1", true),
    ];
    for (input, want) in cases {
        assert_eq!(is_valid(input), *want, "is_valid({:?})", input);
    }
}
