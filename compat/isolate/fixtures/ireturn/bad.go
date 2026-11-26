package p

// ireturn has three messages, and which one it uses depends on how the
// interface arrives: named, a type parameter's constraint, or a bare generic.

type I interface{ M() }

// "Bad returns interface (…)"
func Bad() I {
	return nil
}

// "OfTypeParam returns generic interface (…) of type param T"
func OfTypeParam[T I](v T) T {
	return v
}

// A method has the same shape as a function here — the receiver does not change
// which arm runs, but it does change the position (`func`, not the name).
type S struct{}

func (S) Method() I { return nil }
