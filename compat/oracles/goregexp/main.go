// Command goregexp emits ground truth for guff's port of regexp/syntax's
// parser (crates/guff-staticcheck/src/gostd/regexp.rs, used by SA1000).
//
// Output is one row per corpus pattern:
//
//	<Go-quoted pattern><TAB><hex of pattern><TAB><hex of regexp.Compile error>
//
// An empty third column means Compile succeeded. SA1000 prints the error
// verbatim, so that column is exactly what the port must reproduce.
//
// Unlike the other oracles the result column is hex rather than verbatim, and
// for a reason specific to this parser: syntax.Error.Error() embeds Expr, a raw
// substring of the pattern, between backquotes. That substring can hold a tab, a
// newline, or bytes that are not valid UTF-8 at all — ErrInvalidUTF8's Expr is
// precisely the ill-formed tail. There is no encoding step to lean on, so the
// column is hex and the Rust side compares bytes.
//
// The corpus targets every error return in parse.go, and both halves of each:
// the ErrorCode *and* the Expr, since Expr is a different slice of the input at
// nearly every site (the whole regexp for ErrUnexpectedParen, the two-byte
// escape for ErrInvalidEscape, the repeat operator and its operand for
// ErrInvalidRepeatSize, the empty string for ErrTrailingBackslash). A port that
// picks the right code and the wrong slice fails the golden gate just the same.
//
// The -rust flag emits the Unicode tables the port needs as a Rust source file.
// They are generated for the same reason isprint_table.rs is: unicode.Categories,
// unicode.Scripts and unicode.SimpleFold answer for the Unicode version Go's own
// tables are pinned to. The names decide whether `\p{Foo}` is a finding at all,
// and the ranges feed p.numRunes, which is what ErrLarge is measured against.
//
// Regenerate with ../regen.sh; never hand-edit the output.
package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"regexp"
	"regexp/syntax"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"unicode"
)

// atoms are single pieces of syntax. Each is emitted alone and substituted into
// every wrapper, which is what puts them next to a repetition operator, inside a
// class, inside a group, and after another atom.
var atoms = []string{
	// Ordinary literals and the metacharacters that are literals in some spots.
	"a", "ab", "", " ", "-", "_", "]", "}", "{", "~", "#", "&", "!", "\x00",
	".", "^", "$", "|", "(", ")", "[", "*", "+", "?",

	// Repetition. The braces reach parseRepeat, whose failures fall back to a
	// literal `{` rather than erroring, so both outcomes need coverage.
	"a*", "a+", "a?", "a**", "a++", "a??", "a*+", "a+*", "a*?", "a?*", "a{2}{3}",
	"*", "+", "?", "*a", "+a", "?a", "a*?b", "a??b",
	"a{2}", "a{2,}", "a{2,3}", "a{,3}", "a{}", "a{2", "a{2,", "a{2,3", "a{ }",
	"a{0}", "a{1}", "a{1000}", "a{1001}", "a{0,1000}", "a{0,1001}", "a{3,2}",
	"a{-1}", "a{01}", "a{00}", "a{99999999999}", "a{100000000}", "a{2,1}",
	"{2}", "{2,3}", "{", "{}", "{a}", "a{2}?", "a{2}*", "(a){2}{3}",
	// Nested repeats: the ErrInvalidRepeatSize from repeatIsValid, and the
	// ErrLarge from checkSize, are different codes on nearly the same shape.
	"(a{100}){100}", "(a{100}){100}{100}", "((a{100}){100}){100}",
	"(a{2}){2}", "(a{31}){31}", "(a{32}){32}", "(a{1000}){1000}",
	"a{1000}{1000}{1000}", "(((a{1000}){1000}){1000}){1000}",
	"((((a{1000}){1000}){1000}){1000}){1000}",

	// Groups, including every Perl flag form and the named-capture syntaxes.
	"(a)", "(a", "a)", "()", "(())", "(|)", "(a|b)", "(?:a)", "(?:a", "(?:)",
	"(?i)a", "(?i:a)", "(?-i)a", "(?i-s:a)", "(?im-sU:a)", "(?)", "(?", "(?i",
	"(?-)", "(?--)", "(?-i-s:a)", "(?x)", "(?q)", "(?#c)", "(?=a)", "(?!a)",
	"(?<=a)", "(?<!a)", "(?P=name)", "(?P>name)", "(?'n'a)", "(?R)", "(?0)",
	"(?P<n>a)", "(?P<n>a", "(?P<>a)", "(?P<n", "(?P<1a>x)", "(?P<n-x>a)",
	"(?<n>a)", "(?<>a)", "(?<n", "(?<n>a", "(?<a.b>x)", "(?P<\u00e9>x)",
	"(?P<n>a)(?P<n>b)", "(?<n>a)(?<m>b)", "(?P<_>a)", "(?P<0>a)", "(?<\u00e9>x)",

	// Escapes.
	"\\", "\\a", "\\b", "\\B", "\\A", "\\z", "\\Z", "\\C", "\\Q", "\\E",
	"\\Qab\\E", "\\Qa+b\\E*", "\\Q\\E", "\\Q", "\\Qab", "\\q", "\\_", "\\-",
	"\\n", "\\r", "\\t", "\\f", "\\v", "\\0", "\\1", "\\12", "\\123", "\\7",
	"\\8", "\\9", "\\01", "\\012", "\\0123", "\\777", "\\400",
	"\\x", "\\xa", "\\xag", "\\xff", "\\x{}", "\\x{41}", "\\x{110000}",
	"\\x{10FFFF}", "\\x{FFFFFFFFFF}", "\\x{41", "\\x{g}", "\\x{4 1}", "\\x{0}",
	"\\d", "\\D", "\\s", "\\S", "\\w", "\\W", "\\pL", "\\PL", "\\p{Greek}",
	"\\p{^Greek}", "\\P{^Greek}", "\\p{Foo}", "\\p{}", "\\p{", "\\p", "\\pZ",
	"\\p{Latin}", "\\p{Any}", "\\p{Assigned}", "\\p{ASCII}", "\\p{LC}",
	"\\p{Lc}", "\\p{lc}", "\\p{Greek_And_More}", "\\p{Cased_Letter}",
	"\\p{cased letter}", "\\p{Cased-Letter}", "\\p{Nd}", "\\p{^}", "\\p^",
	"\\p{Han}", "\\pX", "\\p{Latin", "\\p{L}\\p{L}", "(?i)\\p{Greek}",
	"(?i)\\pL", "(?i)\\p{Lu}", "(?i)[\\p{Lu}]", "\\p{Xyz}", "\\P{Foo}",

	// Character classes.
	"[a]", "[^a]", "[]", "[]a]", "[^]a]", "[a", "[^a", "[-]", "[a-]", "[-a]",
	"[a-b]", "[b-a]", "[a-b-c]", "[\\d]", "[\\D]", "[^\\d]", "[\\w-]", "[\\w-.]",
	"[\\d-a]", "[a-\\d]", "[[:alpha:]]", "[[:^alpha:]]", "[[:foo:]]", "[[:alpha]]",
	"[[:]]", "[[::]]", "[[:alpha:]", "[:alpha:]", "[[:word:]]", "[[:punct:]]",
	"[\\p{Greek}]", "[\\p{Foo}]", "[\\pL]", "[\\P{Greek}]", "[\\pFoo]",
	"[a-z]", "[A-Za-z0-9]", "[^\\n]", "[\\n]", "[\\x00-\\x7F]", "[\\]]",
	"[[]", "[[[]", "[]]", "[^]]", "[\\-]", "[a\\-z]", "[.]", "[*]", "[$^]",
	"(?i)[a-z]", "(?i)[k]", "(?i)[\u00e9]", "(?i)[[:alpha:]]", "(?i)[^a]",
	"[\\Qa\\E]", "[\\x{41}-\\x{5A}]", "[\\x{5A}-\\x{41}]", "[z-a]", "[\\s\\S]",
	"[^\\x00-\\x{10FFFF}]", "[\\x00-\\x{10FFFF}]",

	// Alternation and the empty-branch shapes that exercise factor/collapse.
	"a|b", "|", "a|", "|a", "a||b", "|||", "(a|)", "(|a)", "abc|abd|aef",
	"abc|abd|aef|bcx|bcy", "a|b|c|d", "(ab|ac)", "(a|b)*", "x(a|b)|x(c|d)",

	// Anchors and dot.
	"^a$", "\\Aa\\z", "a.b", "(?s)a.b", "(?m)^a$", "\\b\\B",

	// Invalid UTF-8. Written as byte escapes because the ill-formed tail is
	// exactly what ErrInvalidUTF8 puts in Expr.
	"\xff", "a\xff", "\xffa", "\xc3", "\xc3(", "a\xc3\x28b", "\xe2\x82",
	"[\xff]", "[a-\xff]", "(?P<\xff>a)", "\\p{\xff}", "(?P<a\xffb>x)",
	"\\x{41}\xff", "\xff*", "(?\xff)", "\\\xff",
}

// wrappers put an atom in a context. %s is the atom.
var wrappers = []string{
	"%s",
	"a%s",
	"%sa",
	"(%s)",
	"[%s]",
	"%s*",
	"%s{2}",
	"%s|b",
	"b|%s",
	"(?i)%s",
	"(?:%s)",
	"\\Q%s\\E",
	"^%s$",
}

// maxRunesLimit mirrors parse.go's maxRunes (128 MB / 4 bytes per rune), and
// runesPerPC is len(Rune) for one `\pC` class under Go's current Unicode
// tables. Both are asserted against the real parser in main before use, so a
// Go release that moves either produces a loud failure rather than a corpus
// that quietly stops straddling the limit.
const (
	maxRunesLimit = 128 << 20 / 4
	runesPerPC    = 1424
)

// standalone covers what the grid cannot express: the depth and size limits,
// which need generated input, and a handful of long-form shapes.
func standalone() []string {
	rep := strings.Repeat
	return []string{
		// ErrNestingDepth is a panic recovered in parse; its Expr is the whole
		// regexp, so the row is large on purpose. 999/1000/1001 straddle it.
		rep("(", 999) + "a" + rep(")", 999),
		rep("(", 1000) + "a" + rep(")", 1000),
		rep("(", 1001) + "a" + rep(")", 1001),
		rep("(?:", 1001) + "a" + rep(")", 1001),
		rep("(", 2000) + "a" + rep(")", 2000),
		// Unclosed: reaches ErrMissingParen rather than the depth limit, and
		// the two have very different Expr.
		rep("(", 1001) + "a",
		rep("[", 200),
		// Alternation and concatenation nest through different tree paths.
		rep("(a|", 1001) + "b" + rep(")", 1001),
		"a" + rep("|a", 500),
		rep("a", 2000),
		// ErrLarge via checkSize.
		"a{1000}{1000}{1000}{1000}",
		"(a{1000}){1000}",
		"((a{1000}){1000}){1000}",
		"(((a{1000}){1000}){1000}){1000}",
		"((((((a{1000}){1000}){1000}){1000}){1000}){1000})",
		"(?:a{1000}){1000}{1000}",
		// ErrLarge via maxRunes, which is a different counter from checkSize's:
		// it sums len(re.Rune) over every pushed class. `\pC` is the densest
		// class Go has (1424 runes for three bytes of pattern), so these two
		// rows straddle the limit as cheaply as it can be straddled — they are
		// still the largest rows in the file by two orders of magnitude, which
		// is the price of covering a counter whose threshold is 33.5M runes.
		rep("\\pC", maxRunesLimit/runesPerPC),
		rep("\\pC", maxRunesLimit/runesPerPC+1),
		// A long literal alternation is what drives factor's four rounds.
		strings.Join(func() []string {
			var out []string
			for i := 0; i < 200; i++ {
				out = append(out, fmt.Sprintf("ab%02d", i))
			}
			return out
		}(), "|"),
		// Real-world shapes the old approximation carried exceptions for; they
		// are valid in Go and must stay valid.
		`{header\.([\w-]*)}`,
		`{re\.([\w-\.]*)}`,
		`[-\/^$+?.()|[\]{}]`,
		`^(?P<name>[a-z]+)-(?P<n>\d+)$`,
		`(?i)^https?://[^\s/$.?#].[^\s]*$`,
	}
}

func corpus() []string {
	seen := map[string]bool{}
	var out []string
	add := func(s string) {
		if !seen[s] {
			seen[s] = true
			out = append(out, s)
		}
	}
	for _, a := range atoms {
		for _, w := range wrappers {
			add(strings.ReplaceAll(w, "%s", a))
		}
	}
	for _, s := range standalone() {
		add(s)
	}
	return out
}

// ---------------------------------------------------------------- rust tables

type namedTable struct {
	name   string
	ranges [][3]uint32 // lo, hi, stride
}

func flatten(t *unicode.RangeTable) [][3]uint32 {
	var out [][3]uint32
	for _, r := range t.R16 {
		out = append(out, [3]uint32{uint32(r.Lo), uint32(r.Hi), uint32(r.Stride)})
	}
	for _, r := range t.R32 {
		out = append(out, [3]uint32{r.Lo, r.Hi, r.Stride})
	}
	return out
}

func sortedTables(m map[string]*unicode.RangeTable) []namedTable {
	var names []string
	for n := range m {
		names = append(names, n)
	}
	sort.Strings(names)
	out := make([]namedTable, 0, len(names))
	for _, n := range names {
		out = append(out, namedTable{n, flatten(m[n])})
	}
	return out
}

func emitTableMap(w *bufio.Writer, ident, doc string, tables []namedTable) {
	fmt.Fprintf(w, "/// %s\n", doc)
	fmt.Fprintf(w, "pub(super) const %s: &[(&str, &[(u32, u32, u32)])] = &[\n", ident)
	for _, t := range tables {
		fmt.Fprintf(w, "    (%q, &[\n", t.name)
		for _, r := range t.ranges {
			fmt.Fprintf(w, "        (0x%X, 0x%X, %d),\n", r[0], r[1], r[2])
		}
		fmt.Fprintf(w, "    ]),\n")
	}
	fmt.Fprintln(w, "];")
}

func emitRustTables(w *bufio.Writer) {
	fmt.Fprintln(w, "//! Generated by `compat/oracles/regen.sh goregexp-table`; do not edit.")
	fmt.Fprintln(w, "//!")
	fmt.Fprintf(w, "//! The Unicode data `regexp/syntax` consults, as of %s.\n", runtime.Version())
	fmt.Fprintln(w, "//! Two different things ride on it: the *names* decide whether `\\p{Foo}` is")
	fmt.Fprintln(w, "//! an `invalid character class range` finding, and the *ranges* feed")
	fmt.Fprintln(w, "//! `p.numRunes`, which is what `expression too large` is measured against.")
	fmt.Fprintln(w, "//! Both move when Go bumps its Unicode version — read the diff.")
	fmt.Fprintln(w, "#![allow(clippy::unreadable_literal)]")
	fmt.Fprintln(w)

	emitTableMap(w, "CATEGORIES", "`unicode.Categories`, sorted by name.", sortedTables(unicode.Categories))
	fmt.Fprintln(w)
	emitTableMap(w, "SCRIPTS", "`unicode.Scripts`, sorted by name.", sortedTables(unicode.Scripts))
	fmt.Fprintln(w)
	emitTableMap(w, "FOLD_CATEGORY", "`unicode.FoldCategory`, sorted by name.", sortedTables(unicode.FoldCategory))
	fmt.Fprintln(w)
	emitTableMap(w, "FOLD_SCRIPT", "`unicode.FoldScript`, sorted by name.", sortedTables(unicode.FoldScript))
	fmt.Fprintln(w)

	// CategoryAliases: name -> the Categories key it resolves to.
	var aliases []string
	for n := range unicode.CategoryAliases {
		aliases = append(aliases, n)
	}
	sort.Strings(aliases)
	fmt.Fprintln(w, "/// `unicode.CategoryAliases`, sorted by name. The parser canonicalises the")
	fmt.Fprintln(w, "/// key before lookup, so these are stored raw and canonicalised in Rust.")
	fmt.Fprintln(w, "pub(super) const CATEGORY_ALIASES: &[(&str, &str)] = &[")
	for _, n := range aliases {
		fmt.Fprintf(w, "    (%q, %q),\n", n, unicode.CategoryAliases[n])
	}
	fmt.Fprintln(w, "];")
	fmt.Fprintln(w)

	// SimpleFold, as the pairs that are not the identity. `\p{...}` under (?i)
	// and every folded class range walk this orbit.
	fmt.Fprintln(w, "/// `unicode.SimpleFold`: ascending `(rune, SimpleFold(rune))` for every rune")
	fmt.Fprintln(w, "/// the function does not map to itself. Everything else is the identity.")
	fmt.Fprintln(w, "pub(super) const SIMPLE_FOLD: &[(u32, u32)] = &[")
	for r := rune(0); r <= unicode.MaxRune; r++ {
		if f := unicode.SimpleFold(r); f != r {
			fmt.Fprintf(w, "    (0x%X, 0x%X),\n", r, f)
		}
	}
	fmt.Fprintln(w, "];")
}

// -------------------------------------------------------------------- driver

// checkMaxRunesRows verifies that the two `\pC` rows still land on opposite
// sides of maxRunes. They are sized from constants, and a Go release that
// changes either the limit or the size of the Cn/C tables would otherwise leave
// a corpus that still parses but no longer covers the counter.
func checkMaxRunesRows() {
	re, err := syntax.Parse(`\pC`, syntax.Perl)
	if err != nil || len(re.Rune) != runesPerPC {
		fmt.Fprintf(os.Stderr, "runesPerPC is %d but \\pC now has %d runes (err %v)\n",
			runesPerPC, len(re.Rune), err)
		os.Exit(1)
	}
	n := maxRunesLimit / runesPerPC
	if _, err := regexp.Compile(strings.Repeat(`\pC`, n)); err != nil {
		fmt.Fprintf(os.Stderr, "%d copies of \\pC should be under maxRunes, got %v\n", n, err)
		os.Exit(1)
	}
	_, err = regexp.Compile(strings.Repeat(`\pC`, n+1))
	if err == nil || !strings.Contains(err.Error(), string(syntax.ErrLarge)) {
		fmt.Fprintf(os.Stderr, "%d copies of \\pC should exceed maxRunes, got %v\n", n+1, err)
		os.Exit(1)
	}
}

func main() {
	rustTables := flag.Bool("rust", false, "emit the Unicode tables as a Rust source file")
	flag.Parse()

	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	if *rustTables {
		emitRustTables(w)
		return
	}
	checkMaxRunesRows()

	for _, s := range corpus() {
		msg := ""
		if _, err := regexp.Compile(s); err != nil {
			msg = err.Error()
		}
		fmt.Fprintf(w, "%s\t%x\t%x\n", strconv.Quote(s), s, msg)
	}
}
