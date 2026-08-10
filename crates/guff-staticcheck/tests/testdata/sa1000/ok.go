// Patterns Go accepts. Most of these are here because the approximation this
// check used to be — the Rust `regex` crate's parser, with hand-written
// rewrites in front of it — reported them: it accepts a different *set* of
// inputs than RE2 does, so precision was as broken as the message text.
package main

import "regexp"

func main() {
	regexp.MustCompile(`(abc)`)
	regexp.Compile(`ok`)

	// Braces that do not form a quantifier are literals in Go, not a syntax
	// error. Caddy's placeholder patterns are full of them.
	regexp.MustCompile(`{header\.([\w-]*)}`)
	regexp.MustCompile(`{re\.([\w-\.]*)}`)
	regexp.MustCompile(`a{,3}`)
	regexp.MustCompile(`a{}`)
	regexp.MustCompile(`{2,3`)

	// A perl class is allowed where a range endpoint would go: RE2 reads
	// `[\w-.]` as the set, not as a range starting at `\w`.
	regexp.MustCompile(`[\w-.]`)
	regexp.MustCompile(`[\w-]`)
	// Grafana's cloud-monitoring wildcard escaper: `[` inside a class is a
	// literal, not the start of a nested one.
	regexp.MustCompile(`[-\/^$+?.()|[\]{}]`)

	// \Q...\E quotes its contents, including the metacharacters, and an
	// unterminated \Q runs to the end of the pattern.
	regexp.MustCompile(`\Qa+b\E`)
	regexp.MustCompile(`\Q\E`)
	regexp.MustCompile(`\Qab`)

	// Classes, groups and flags that are valid but reach the more elaborate
	// paths: fold-case merging, the POSIX table, the Unicode tables, and the
	// alternation factoring.
	regexp.MustCompile(`(?i)[a-z]`)
	regexp.MustCompile(`(?i)\p{Greek}`)
	regexp.MustCompile(`\p{Latin}`)
	regexp.MustCompile(`\p{Assigned}`)
	regexp.MustCompile(`[[:word:]]`)
	regexp.MustCompile(`[^[:^alpha:]]`)
	regexp.MustCompile(`(?im-sU:a)`)
	regexp.MustCompile(`abc|abd|aef|bcx|bcy`)

	// The boundaries: 1000 repeats is the largest Go allows, and `]` first in
	// a class is a literal.
	regexp.MustCompile(`a{1000}`)
	regexp.MustCompile(`[]a]`)
	regexp.MustCompile(`[a-]`)

	// Well-formed UTF-8 for the same code points the byte escapes above name.
	// `ÿ` is two bytes and compiles; `\xff` is one and does not.
	regexp.MustCompile("ÿ")
	regexp.MustCompile("\xc3\xbf")
	regexp.MustCompile("a☃b")

	regexp.MustCompile(`^(?P<name>[a-z]+)-(?P<n>\d+)$`)
	regexp.Match(`\d+`, nil)
	regexp.MatchReader(`\pL`, nil)
	regexp.MatchString(`(?s)a.b`, "")
}
