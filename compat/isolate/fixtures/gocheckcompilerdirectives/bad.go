package p

// go:embed x.txt
//go:genrate echo hi
var _ string

// The linter says several different things about a malformed directive: an
// unknown one, one with a space after the slashes, and a misspelled prefix.
//go:nosuchdirective
func Unknown() {}

// go:noinline
func MissingSlashes() {}

//gO:noinline
func WrongCase() {}
