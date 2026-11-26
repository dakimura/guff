package main

func f(s string) {
	for _, r := range s {
		_ = r
	}
}

func g(x []rune) {
	for _, r := range x {
		_ = r
	}
}
