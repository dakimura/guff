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

// The first regular field's *doc comment* is where upstream inserts when there
// is one — `firstRegularField.Doc.Pos()`, not the field's own position. Nothing
// here exercised that branch until 2026-08-27 (COMPAT-HARDENING 続き 79).
type DocumentedFirst struct {
	A
	B
	// V is documented, so the blank line belongs above this comment.
	V int
}
