package main

type T struct{ X int }

func (v T) fn() {
	v.X = 1
}

func main() {
	var t T
	t.fn()
}
