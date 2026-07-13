package main
type A interface { M() }
type B interface { M() }
func f(a A) { _ = a.(B) }
