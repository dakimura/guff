package main

func f(x int) int {
	y := x
	x = 1
	return y
}

// Use in condition before overwrite must not be flagged.
func g(x int) int {
	if x == 0 {
		x = 1
	}
	return x
}

func h(st *int) *int {
	if st == nil {
		x := 0
		st = &x
	}
	return st
}

func main() {}
