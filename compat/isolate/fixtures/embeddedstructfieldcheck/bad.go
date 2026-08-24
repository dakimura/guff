package p

type A struct{ X int }
type B struct{ Y int }

type Bad struct {
	A
	Z int
	B
}

// The linter reports the embedded field, so each one out of place is its own
// sentence — and a struct whose embeds are all first is the negative.
type AlsoBad struct {
	W int
	A
}

type Ok struct {
	A
	B
	V int
}
