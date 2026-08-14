package main

import "math"

// None of these is a conversion from an integer, and upstream reports none.
func main() {
	var x float64
	_ = math.Ceil(1)          // constant, already float64 — no Convert
	_ = math.Ceil(1.5)        // ditto
	_ = math.Trunc(x)         // a genuine float
	_ = math.Ceil(float64(x)) // a conversion, but from float64
}
