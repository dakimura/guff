package main

type T1 struct{ A int; B string }
type T2 struct{ A int; B string }

func f(x T1) T2 {
	return T2{A: x.A, B: x.B}
}
