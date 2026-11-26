package group

// A is documented here. This line will recieve a misspelling, and the next one
// carries another recieve so the range has to cover more than one line.
//
// The directive is on the last line of the group, but the suppression range is
// built from the *group*, so the prose above is covered too — and the range
// expander then stretches it over the function below.
//
//nolint:misspell // the upstream enum is spelled that way
func A() {
	// Inside the function body: still covered, we recieve that too.
}

// B is documented here, in a comment group of its own: this recieve is
// reported.
func B() {}
