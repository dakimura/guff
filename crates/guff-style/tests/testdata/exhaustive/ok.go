package exhaustive

type Token int

const (
	A Token = iota
	B
	C
)

func ok(t Token) {
	switch t {
	case A:
	case B:
	case C:
	}
}

func okDefault(t Token) {
	// default does NOT satisfy exhaustiveness by default — list all.
	switch t {
	case A, B, C:
	default:
	}
}
