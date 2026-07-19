package main

func usedAfterInc() {
	x := 1
	_ = x
	x = 2
	_ = x
}

func loopClassic(n int) int {
	sum := 0
	for i := 0; i < n; i++ {
		sum += i
	}
	return sum
}

func loopBodyInc(n int) int {
	x := 0
	for x < n {
		x++
	}
	return x
}

func usedInc() {
	var n int
	n++
	println(n)
}

func usedAdd() {
	n := 1
	n += 1
	println(n)
}
