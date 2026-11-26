package p

func process() {
	// This is is a duplicate
	line := "the the word"
	_ = line
}

// dupword names the word it saw twice, so each duplicated word is a different
// sentence. It reads comments, strings and the raw source alike.
func more() {
	// and and again
	s := "a a b"
	_ = s
	/* block block comment */
}
