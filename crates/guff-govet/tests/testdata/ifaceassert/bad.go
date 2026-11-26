package p

type I interface {
	M() int
}

type J interface {
	M() string
}

func f(x I) {
	_ = x.(J)
}
