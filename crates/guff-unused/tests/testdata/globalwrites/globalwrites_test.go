package globalwrites

// Declared in a test file and written there: (4.9) keeps it. `init` stands in
// for the benchmark upstream has in mind — what the rule looks at is the file
// the variable is *declared* in, not what does the writing, and the unit-test
// type-checker has no importer for `testing`.
var benchSink int

// Declared in a test file and never touched at all.
var neverTouched int

func init() {
	// A write from a test file to a global declared in a non-test file: the
	// exception does not reach it.
	writtenFromTest = 1
	benchSink = 2
}
