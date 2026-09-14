// Package escapes is goconst's value: the string after `strconv.Unquote`, not
// the characters between the quotes.
//
// Upstream:
//
//	if unquotedStr, err = strconv.Unquote(str); err != nil {
//		unquotedStr = str[1 : len(str)-1]   // manually strip quotes
//	}
//	if len(unquotedStr) == 0 || utf8.RuneCountInString(unquotedStr) < v.p.minLength {
//		return
//	}
//
// That value is both the length being measured and the map key, so a
// hand-rolled unquote that leaves `\xc5` as four characters gets `min-len`
// *and* grouping wrong.
package escapes

// silent — one rune after unquoting, under the default min-len of 3. This is
// pyroscope `pkg/validation/validate_test.go:71`.
func oneRune() []string { return []string{"\xc5", "\xc5", "\xc5"} }

// fires once with six occurrences — `\x61bc` and `abc` are the same string.
// Counting the written form makes it two strings of three, which is under
// min-occurrences and reports nothing at all.
func spelledTwoWays() []string {
	return []string{"\x61bc", "\x61bc", "\x61bc", "abc", "abc", "abc"}
}

// fires — a raw string and an escaped one, three occurrences of `a<TAB>b`.
func rawAndEscaped() []string {
	return []string{"a\tb", "a\tb", `a` + "\t" + `b`, "a\tb"}
}

var _ = []any{oneRune, spelledTwoWays, rawAndEscaped}
