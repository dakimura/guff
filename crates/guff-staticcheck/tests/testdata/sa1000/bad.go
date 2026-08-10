// Every ErrorCode regexp/syntax can reach from a pattern short enough to write
// as a source literal, and both halves of each finding: the code, and the Expr
// the message quotes after it — which is a different slice of the pattern at
// nearly every site.
//
// Not here, and why:
//   - invalid UTF-8: guff's constant layer cannot deliver the bytes (see
//     docs/COMPAT-HARDENING.md §5). Upstream reports it; guff stays silent.
//   - expression too large / expression nests too deeply: both need multi-kB
//     literals. compat/oracles/goregexp covers them instead.
package main

import "regexp"

func main() {
	// Structural: the Expr is the whole pattern for the paren errors and just
	// the bracket for the class one.
	regexp.Compile(`foo(`)
	regexp.MustCompile(`[`)
	regexp.MustCompile(`a)`)

	// Repetition. Expr spans the operator together with what follows it, so
	// `a**` reports `**` and not `*`.
	regexp.MustCompile(`*`)
	regexp.MustCompile(`a**`)
	regexp.MustCompile(`a{2,1}`)
	regexp.MustCompile(`a{1001}`)
	// This one is not a bounds check but a rewalk of the tree: 100 copies of
	// 100 copies exceeds repeatIsValid's budget of 1000.
	regexp.MustCompile(`(a{100}){100}`)

	// Escapes. \C is rejected in its own arm of the big switch, before the
	// ordinary escape path, and \ at the end reports an empty Expr.
	regexp.MustCompile(`\q`)
	regexp.MustCompile(`\C`)
	regexp.MustCompile("\\")

	// Character classes: a reversed range, an unknown POSIX name, and an
	// unknown Unicode class all share one code and quote three different spans.
	regexp.MustCompile(`[z-a]`)
	regexp.MustCompile(`[[:foo:]]`)
	regexp.MustCompile(`\p{Foo}`)

	// Named captures: empty, and a name outside [A-Za-z0-9_]. The Expr stops
	// at the `>`, so it is the opening of the group rather than all of it.
	regexp.MustCompile(`(?P<>a)`)
	regexp.MustCompile(`(?<a.b>x)`)

	// Lookahead is Perl syntax Go does not implement.
	regexp.MustCompile(`(?=a)`)

	// The other four call sites upstream matches on.
	regexp.Match(`+`, nil)
	regexp.MatchReader(`(?P<>x)`, nil)
	regexp.MatchString(`[a-\d]`, "")
}
