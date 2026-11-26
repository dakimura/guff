// Port of Go's go/token/token_test.go to Rust.
//
// Original: Copyright 2019 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.

use guff::is_identifier;

#[test]
fn test_is_identifier() {
    let tests: &[(&str, &str, bool)] = &[
        ("Empty", "", false),
        ("Space", " ", false),
        ("SpaceSuffix", "foo ", false),
        ("Number", "123", false),
        ("Keyword", "func", false),
        ("LettersASCII", "foo", true),
        ("MixedASCII", "_bar123", true),
        ("UppercaseKeyword", "Func", true),
        ("LettersUnicode", "fóö", true),
    ];

    for (name, input, want) in tests {
        let got = is_identifier(input);
        assert_eq!(
            got, *want,
            "{}: IsIdentifier({:?}) = {}, want {}",
            name, input, got, want
        );
    }
}
