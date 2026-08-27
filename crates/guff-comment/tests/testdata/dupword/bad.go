package example

func process() {
	// This is is a comment with duplicate words
	line := "the the duplicate"
	_ = line
}

// The literal half of dupword carried no fix until 2026-08-27, and these
// shapes were what the fixture never asked about (COMPAT-HARDENING 続き 74).
// Upstream checks the *unquoted* text and re-quotes the rewrite, so an escape
// is compared as the byte it stands for and the quoting style can change.
func literals() {
	// The tab is a real tab by the time the scan runs, and `strconv.Quote`
	// escapes it again on the way back.
	tabbed := "the\tthe word"
	newline := "a\na b"

	// `\\` unquotes to one backslash, so `the\the` is a single word and there
	// is nothing to report.
	escaped := "the\\the word"

	// The duplicate sits inside an escaped quote, and is not reported.
	quoteInside := "say \"the the\" now"

	// A raw string is rewritten as an *interpreted* one: upstream re-quotes
	// with `strconv.Quote`, which has no way to spell a backtick literal. The
	// backticks are gone from the fixed file.
	raw := `the the word`

	// ...but a raw string's `\t` is two characters, so `the\tthe` is one word
	// and this one is silent — the pair above and below only differ by quoting.
	rawEsc := `the\tthe word`

	// Non-ASCII, to pin that the scan is not byte-indexed.
	uni := "é é x"

	// No trailing text after the duplicate.
	runes := "the the"

	_, _, _, _, _, _, _, _ = tabbed, newline, escaped, quoteInside, raw, rawEsc, uni, runes
}
