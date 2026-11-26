package p

// Reveal reaches T's unexported field, so it can only live in package p — and
// it is wanted only by p's external test package. That is what export_test.go
// is for, and it exists only in p's test variant.
func Reveal(t T) int { return t.x }
