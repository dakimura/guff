package exhaustive

type Token int

const (
	A Token = iota
	B
	C
)

func bad(t Token) {
	switch t { // want missing C
	case A:
	case B:
	}
}
