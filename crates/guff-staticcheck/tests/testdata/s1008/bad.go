package main

func fn() bool { return true }

func fn1() bool {
	x := true
	if x {
		return true
	}
	return false
}

func fn2() bool {
	if fn() {
		return false
	}
	return true
}

func fn3(x int) bool {
	if x > 0 {
		return false
	}
	return true
}

func fn4(x bool) bool {
	if x {
		return false
	}
	return true
}

func fn5(x string) bool {
	if len(x) > 0 {
		return false
	}
	return true
}
