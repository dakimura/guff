package p

// whitespace has three messages. The two newline ones are the common pair; the
// multi-line one needs `multi-if` / `multi-func` turned on.

func LeadingNewline() {

	_ = 1
}

func TrailingNewline() {
	_ = 1

}

func MultiLineIf(a, b bool) {
	if a &&
		b {
		_ = 1
	}
}
