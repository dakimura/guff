// Command gotemplate emits ground truth for guff's port of text/template's
// parser (crates/guff-staticcheck/src/gostd/template.rs, used by SA1001).
//
// Output is three sections:
//
//	letter<TAB><lo hex><TAB><hi hex>   one row per maximal run of unicode.IsLetter
//	digit<TAB><lo hex><TAB><hi hex>    same for unicode.IsDigit
//	parse<TAB><hex of input><TAB><text/template error><TAB><html/template error>
//
// An empty error column means Parse succeeded. SA1001 prints the error
// verbatim, so those two columns are exactly what the port must reproduce; it
// reports only errors containing "unexpected" or "bad character", but a port
// that stops at a *different* error than Go does reports where Go stays quiet,
// so every message is compared, not just the two reported classes.
//
// The letter/digit sections exist for the same reason the print section of
// goquote does: template identifiers are delimited by unicode.IsLetter and
// unicode.IsDigit, which answer for the Unicode version Go's tables are pinned
// to. A Rust category crate on any other version disagrees, so guff carries a
// copy of Go's tables (emitted by the -rust flag) and checks them here over the
// whole rune space.
//
// Regenerate with ../regen.sh; never hand-edit the output.
package main

import (
	"bufio"
	"flag"
	"fmt"
	htmltemplate "html/template"
	"os"
	"runtime"
	"strings"
	texttemplate "text/template"
	"unicode"
	"unicode/utf8"
)

// bodies go between the action delimiters. They are chosen to reach every
// error return in lex.go and parse.go: each lexer state (number, quote, raw
// quote, char constant, field, variable, identifier, paren), each keyword, the
// declaration forms, and the shapes that terminate an identifier badly.
var bodies = []string{
	"", " ", ".", "$", "$x", "$x.y", "$.x", "$x := 1", "$x = 1", "$x, $y := 1",
	"$x, $y, $z := 1", "$x,", "$x :", "$x :=", "$1", "$x 1", "$ x",
	".x", ".X.Y", ".x.", "..", "..x", ".x .y", ".x-y", ".1", ".1x", ".x|",
	"1", "-1", "+1", "1.2", "1e", "1e5", "0x1p2", "089", "1_0", "0b12", "0o8",
	"1i", "1+2i", "1+2", "1.2.3", "99999999999999999999", "1x", "0x", "1.2e",
	// The complex forms reach fmt.Sscan, which is where text/template gets its
	// number errors from: floatToken decides the split, ParseFloat the value.
	"0x1p2+3i", "0b1+1i", "0x1+2i", "1e999+1i", "1+1e999i", "1e-999+1i",
	"0x1p999999+1i", "1_0.5+2i", "1.+2i", ".5+.5i", "1+2I", "1++2i",
	"'a'", "'\\n'", "'", "'ab'", "'\\x'", "''", "'\\u00e9'", "'\\''", "'\\\\'",
	"'\\x41'", "'\\101'", "'\\777'", "'\\uD800'", "'\\U0001F600'",
	`"s"`, `"s`, `"\q"`, `""`, "`raw`", "`raw", "``", "`a\rb`",
	`"\x41"`, "\"\\u00e9\"", `"\400"`, `"a\"b"`, `"\U0001F600"`,
	"nil", "true", "false", "nil.x", "true.x", `"a".x`, "1.x", "..y",
	"if", "if .", "if", "end", "else", "else if .", "range", "range .",
	"range $i, $v := .", "range $i := .", "with", "with .", "break", "continue",
	"template", `template "n"`, `template "n" .`, `template n`, `template "n" . .`,
	"block", `block "n"`, `block "n" .`, "define", `define "n"`,
	"print 1", "print", "printf", "undefinedfn", "and 1 2", "not", "len .",
	"(", ")", "(1)", "((1)", "1)", "(.x)", "()", "(1))", "((((1))))",
	"| print", "print |", ".x | print", "1 | print", `"a" | print`, "print | 1",
	"=", ":=", ":", ",", ";", "#", "@", "\\", "%", "-", "- 3", "-3", "!",
	"1 2", `"a" "b"`, "1 . 2", ".x 1", "print .x .y",
	// Non-ASCII bodies are written as escapes: isAlphaNumeric is
	// unicode.IsLetter/IsDigit, so these decide where a name ends, and a
	// literal combining mark or zero-width space would be invisible here.
	"\u00e9", "\u00dcnicode", "a\u0301", ".\u00e9", "$\u00e9", "\u4e16\u754c",
	"\u0660", "1\u0660", "x\u200b", "x\u00a0y", "\u00a0",
	"\t", "\n", " \n ", "\r", "\v", "\x00", "x\x00y",
}

// wrappers place a body in a context: bare, nested in a control structure,
// inside a define, unterminated, and with trim markers.
var wrappers = []string{
	"{{%s}}",
	"text {{%s}} text",
	"{{%s",
	"{{%s}",
	"{{%s}}}",
	"{{- %s -}}",
	"{{if .}}{{%s}}{{end}}",
	"{{range .}}{{%s}}{{end}}",
	`{{define "d"}}{{%s}}{{end}}`,
	"{{print (%s)}}",
	"{{%s | print}}",
}

// standalone shapes the grid cannot express: nesting, comments, delimiters,
// trim markers, and the tree-set errors.
var standalone = []string{
	"", "plain text", "{", "}", "{{", "}}", "{{}}", "{{{{}}}}", "{}{}", "{{{}}}",
	"{{ }}", "{{\t}}", "{{\n}}", "{{}}{{}}", "a{{b}}c",
	"{{/* c */}}", "{{/* c", "{{/* c */", "{{/* c */ }}", "{{/*c*/}}x",
	"{{- /* c */ -}}", "{{/* {{ */}}", "{{/**/}}", "{{/*/*/}}", "{{ /* c */ }}",
	"{{if .}}", "{{end}}", "{{else}}", "{{if .}}{{end}}", "{{if .}}{{else}}{{end}}",
	"{{if .}}{{else if .}}{{end}}", "{{if .}}{{else if .}}{{else}}{{end}}",
	"{{if .}}{{else with .}}{{end}}", "{{with .}}{{else with .}}{{end}}",
	"{{if .}}{{else}}{{else}}{{end}}", "{{if .}}{{end}}{{end}}",
	"{{range .}}{{end}}", "{{range .}}{{break}}{{end}}", "{{break}}",
	"{{range .}}{{continue}}{{end}}", "{{continue}}", "{{range .}}{{else}}{{end}}",
	"{{range .}}{{if .}}{{break}}{{end}}{{end}}", "{{if .}}{{break}}{{end}}",
	"{{with .}}{{end}}", "{{with .}}{{else}}{{end}}", "{{block \"b\" .}}{{end}}",
	"{{block \"b\" .}}", "{{block \"b\"}}{{end}}", "{{block .}}{{end}}",
	`{{define "d"}}{{end}}`, `{{define "d"}}`, `{{define "d"}}x{{end}}`,
	`{{define "d"}}x{{end}}{{define "d"}}y{{end}}`,
	`{{define "d"}}{{end}}{{define "d"}}{{end}}`,
	`{{define ""}}x{{end}}`, `{{define ""}}x{{end}}y`,
	`{{define "d"}}{{else}}{{end}}`, `{{define d}}{{end}}`, `{{define "d"}}{{end}}{{end}}`,
	`{{template "t"}}`, `{{template "t" .}}`, `{{template}}`, `{{template .}}`,
	"{{$x := 1}}{{$x}}", "{{$x}}", "{{range $i, $v := .}}{{$i}}{{end}}",
	"{{$x := 1}}{{if .}}{{$x}}{{end}}", "{{if .}}{{$x := 1}}{{end}}{{$x}}",
	"{{range $i, $v, $w := .}}{{end}}", "{{if $x, $y := 1}}{{end}}",
	"{{with $x := .}}{{end}}", "{{$x := }}", "{{:= 1}}",
	"{{- 3}}", "{{-3}}", "{{3 -}}", "{{3-}}", "{{- -}}", "{{--}}", "{{-}}",
	"a {{- .x -}} b", "a\n{{- .x}}", "{{.x -}}\nb", "{{- if .}}{{- end}}",
	"{{print `a` `b`}}", "{{print \"a\\nb\"}}", "{{print '\\''}}",
	"{{.x.y.z}}", "{{$x.y.z}}", "{{(.x).y}}", "{{(1).y}}", "{{(print 1).x}}",
	"{{print .x | print .y | print}}", "{{1 | print | print}}",
	"{{print (print (print 1))}}", "{{print ()}}", "{{()}}",
	"{{if}}{{end}}", "{{if .x}}{{end}}", "{{range}}{{end}}", "{{with}}{{end}}",
	"{{end .}}", "{{else .}}", "{{end 1}}", "{{else 1}}",
	// item.String truncates with %.10q: the length test counts bytes and the
	// truncation counts runes, so the second of these keeps all seven of its
	// runes and still gains the ellipsis.
	`{{end "0123456789abc"}}`, "{{end \"\u00e9\u00e9\u00e9\u00e9\u00e9\"}}",
	"{{end 12345678901234567890}}", "{{end .0123456789abc}}",
	// The comma branch of a range declaration, which wants a variable next.
	"{{range $i, 1 := .}}{{end}}", "{{range $i, .x := .}}{{end}}",
	"{{range $i, $v := .}}{{end}}", "{{if $x, $y := 1}}{{end}}",
	"{{eq 1 2}}", "{{index . 1}}", "{{len .}}", "{{call .}}", "{{js .}}",
	"{{html .}}", "{{urlquery .}}", "{{slice . 1}}", "{{printf \"%d\" 1}}",
	"\u00e9{{.x}}\u00e9", "{{.\u00e9}}", " {{.x}}", "{{.x}} ", "{{.x}}\n",
	strings.Repeat("{{if .}}", 20) + strings.Repeat("{{end}}", 20),
	strings.Repeat("(", 30) + "1" + strings.Repeat(")", 30),
	"{{print " + strings.Repeat("(", 40) + "1" + strings.Repeat(")", 40) + "}}",
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
	for _, b := range bodies {
		for _, w := range wrappers {
			add(fmt.Sprintf(w, b))
		}
	}
	for _, s := range standalone {
		add(s)
	}
	return out
}

func parseErr(kind, s string) (string, error) {
	var err error
	switch kind {
	case "text":
		_, err = texttemplate.New("").Parse(s)
	case "html":
		_, err = htmltemplate.New("").Parse(s)
	}
	if err == nil {
		return "", nil
	}
	msg := err.Error()
	if strings.ContainsAny(msg, "\t\n") {
		// The Rust side reads a tab-separated line; a message carrying either
		// would silently truncate the expectation.
		return "", fmt.Errorf("%s error text contains a separator for %q: %q", kind, s, msg)
	}
	return msg, nil
}

// runeRanges walks the whole rune space and hands each maximal run of the
// predicate to emit. Surrogates are excluded: they are not scalar values and
// Rust has no char for them, so the Rust side cannot ask the question.
func runeRanges(pred func(rune) bool, emit func(lo, hi rune)) {
	lo := rune(-1)
	flush := func(hi rune) {
		if lo >= 0 {
			emit(lo, hi)
			lo = -1
		}
	}
	for r := rune(0); r <= utf8.MaxRune; r++ {
		if pred(r) && !(r >= 0xD800 && r <= 0xDFFF) {
			if lo < 0 {
				lo = r
			}
		} else {
			flush(r - 1)
		}
	}
	flush(utf8.MaxRune)
}

func tsvRanges(w *bufio.Writer, section string, pred func(rune) bool) {
	runeRanges(pred, func(lo, hi rune) {
		fmt.Fprintf(w, "%s\t%x\t%x\n", section, lo, hi)
	})
}

// emitRustTable writes the letter and digit ranges as a Rust source file, for
// the reason spelled out in the package comment.
func emitRustTable(w *bufio.Writer) {
	fmt.Fprintln(w, "//! Generated by `compat/oracles/regen.sh gotemplate-table`; do not edit.")
	fmt.Fprintln(w, "//!")
	fmt.Fprintf(w, "//! `unicode.IsLetter` and `unicode.IsDigit` ranges as of %s. These two\n", runtime.Version())
	fmt.Fprintln(w, "//! predicates delimit every identifier, field and variable in a template, so")
	fmt.Fprintln(w, "//! regenerating under a Go release that bumps Unicode moves the boundary")
	fmt.Fprintln(w, "//! between a parsed name and a `bad character` error — read the diff.")
	fmt.Fprintln(w)
	for i, tbl := range []struct {
		name string
		pred func(rune) bool
	}{
		{"LETTER_RANGES", unicode.IsLetter},
		{"DIGIT_RANGES", unicode.IsDigit},
	} {
		if i > 0 {
			fmt.Fprintln(w) // blank line between tables, none trailing: rustfmt
		}
		fmt.Fprintf(w, "/// Inclusive `(lo, hi)` pairs, ascending and non-overlapping.\n")
		fmt.Fprintf(w, "pub(super) const %s: &[(u32, u32)] = &[\n", tbl.name)
		runeRanges(tbl.pred, func(lo, hi rune) {
			fmt.Fprintf(w, "    (0x%X, 0x%X),\n", lo, hi)
		})
		fmt.Fprintln(w, "];")
	}
}

func main() {
	rustTable := flag.Bool("rust", false, "emit the letter/digit ranges as a Rust source file")
	flag.Parse()

	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	if *rustTable {
		emitRustTable(w)
		return
	}

	tsvRanges(w, "letter", unicode.IsLetter)
	tsvRanges(w, "digit", unicode.IsDigit)

	for _, s := range corpus() {
		text, err := parseErr("text", s)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		html, err := parseErr("html", s)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Fprintf(w, "parse\t%x\t%s\t%s\n", s, text, html)
	}
}
