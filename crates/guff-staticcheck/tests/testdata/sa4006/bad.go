package main

func unusedInc() {
	var n int
	n++ // want
}

func unusedAdd() {
	n := 1
	n += 1 // want
}

func overwritten() {
	var n int
	n = 1 // want
	n = 2
	_ = n
}
