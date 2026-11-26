package p

type I interface {
	M() int
}

type J interface {
	M() int
}

func f(x I) {
	_ = x.(J)
}
