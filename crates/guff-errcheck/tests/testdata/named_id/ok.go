package named_id

// Mirrors restic's ID + sibling *Error types that previously made guff's
// errcheck *T Implements probe false-positive on discarded ID returns.
type ID [32]byte

func (id ID) String() string { return "" }

func (id ID) MarshalJSON() ([]byte, error) { return nil, nil }

func (id *ID) UnmarshalJSON(b []byte) error { return nil }

type MultipleIDMatchesError struct{ prefix string }

func (e *MultipleIDMatchesError) Error() string {
	return e.prefix
}

func makeID() ID { return ID{} }

func use() {
	makeID()
	_ = &MultipleIDMatchesError{prefix: "ab"}
}
