package main

func f(x bool) {
	if x == true {
		_ = x
	}
	if x == false {
		_ = x
	}
}
