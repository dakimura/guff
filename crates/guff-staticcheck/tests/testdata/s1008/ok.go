package main

func fn1() bool {
	x := true
	if x {
		return true
	}
	return true
}

func fn2() bool {
	x := true
	if !x {
		return true
	}
	if x {
		return true
	}
	return false
}

func fn3() int {
	var x bool
	if x {
		return 1
	}
	return 2
}

const a = true
const b = false

func fn4(x bool) bool {
	if x {
		return a
	}
	return b
}
