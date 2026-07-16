package pkg

import "math"

func fn() {
	var x float64
	_ = 1.0
	_ = x
	_ = x * x
	_ = x * x * x
	_ = math.Pow(x, 6)
}
