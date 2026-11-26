package main
type A interface { M() }
type B interface { M() int }
func f(a A) { _ = a.(B) }
