package types

// Enough of `go/types` for the dupArg fixture: the checker only needs the two
// predicate functions and a type to pass to them.

type Type interface {
	Underlying() Type
	String() string
}

func Identical(x, y Type) bool           { return false }
func IdenticalIgnoreTags(x, y Type) bool { return false }
