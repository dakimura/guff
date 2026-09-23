// Package globalwrites is honnef's (9.7) and (4.9): a write is not a use.
//
// "variable *reads* use variables, writes do not, except in tests" — a
// package-level variable that is only ever assigned to is unused, however many
// times it is written. The exception is a global **declared in a `_test.go`
// file**, which a write keeps alive (benchmark sinks). Every shape below was
// measured against golangci-lint 2.12.2.
package globalwrites

// Written from an init, never read.
var writtenInInit func(uintptr)

// Written from an ordinary function, never read.
var writtenInFunc func(uintptr)

// Written and read.
var writtenAndRead func(uintptr)

// Only read.
var onlyRead = 1

// `+=` is a write like any other: upstream never looks at the assignment's
// token, so this is not also a read.
var compoundAssigned int

// `x++` likewise, unless `post-statements-are-reads` is on.
var incremented int

// Its address is taken, which reads it.
var addressTaken int

// The key of a range statement is a write.
var rangeKey int

// Declared here, written only from the test file: the (4.9) exception asks
// about the file holding the *declaration*, so it does not apply.
var writtenFromTest int

// A struct whose field is written: the selector's base is read.
type box struct{ n int }

var fieldWritten box

func init() {
	writtenInInit = func(uintptr) {}
	writtenAndRead = func(uintptr) {}
}

// Setter writes without reading.
func Setter() {
	writtenInFunc = func(uintptr) {}
	compoundAssigned += 1
	incremented++
	fieldWritten.n = 1
	for rangeKey = range 3 {
	}
}

// Reader reads.
func Reader() int {
	writtenAndRead(0)
	p := &addressTaken
	*p = 2
	return onlyRead
}
