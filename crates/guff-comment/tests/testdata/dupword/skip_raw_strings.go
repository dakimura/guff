package dupword

// `skip-raw-strings` (dupword v0.1.8, golangci-lint 2.14.0) leaves backquoted
// literals alone and nothing else. Adapted from golangci-lint's
// pkg/golinters/dupword/testdata/dupword_skip_raw_strings.go.

func skipRawStrings() (string, string, string) {
	// this line include duplicated word the the
	raw := `this line include duplicated word the the`
	interpreted := "this line include duplicated word the the"
	multiline := `
SELECT the the column
FROM table`
	return raw, interpreted, multiline
}

// Text that ends in whitespace (dupword v0.1.8): the final word and the
// trailing space are written after the loop. Upstream asks whether the last
// *byte*, read as a Latin-1 rune, is a space — so `à` (C3 A0) counts as one.
func trailingWhitespace() (string, string, string) {
	return "the the end ", "end the the ", "voilà voilà"
}
