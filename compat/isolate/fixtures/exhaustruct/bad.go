package p

type T struct {
	A int
	B string
	C bool
}

// exhaustruct has two messages that differ only in plural: one missing field
// names it, two or more are joined into a list. Both are reported at the
// composite literal's *type*, not its brace.

// "T is missing field B"
func MissingOne() T {
	return T{A: 1, C: true}
}

// "T is missing fields B, C"
func MissingTwo() T {
	return T{A: 1}
}

// A nested literal and a pointer literal are separate composite literals, so
// each is its own finding at its own type.
type Outer struct {
	In T
	N  int
}

func Nested() Outer {
	return Outer{In: T{A: 1, B: "x", C: true}}
}

func Pointer() *T {
	return &T{A: 1, B: "x"}
}
