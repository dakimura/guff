package p

import "errors"

// errname has three messages, and which one fires depends on whether the thing
// named is a sentinel value or a type, and on whether one form or several are
// acceptable for that name.

// "the error type name `%s` should conform to the `%s` format" — a type whose
// name has no acceptable variant beyond one.
type BadErrorType struct{}

func (BadErrorType) Error() string { return "bad" }

// "the sentinel error name `%s` should conform to the `%s` format"
var EndOfFileError = errors.New("eof")

// A method on a pointer receiver names its type the same way; upstream reports
// the *type*, so the position is the TypeSpec either way.
type notMatchingError struct{}

func (*notMatchingError) Error() string { return "x" }

// The third message names *several* acceptable forms rather than one: a type
// whose name could conform in more than one way gets the plural sentence.
type errNotMatching struct{}

func (errNotMatching) Error() string { return "x" }

var badSentinel = errors.New("x")
