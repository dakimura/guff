package main

func f(x bool) {
	if x {
		_ = x
	}
	if !x {
		_ = x
	}
}
