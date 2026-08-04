package p

type T struct {
	A int
	B string
}

func Bad() T {
	return T{A: 1}
}
