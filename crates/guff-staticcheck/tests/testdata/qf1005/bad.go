package pkg

import "math"

func fn() {
	var x float64
	_ = math.Pow(x, 0)
	_ = math.Pow(x, 1)
	_ = math.Pow(x, 2)
	_ = math.Pow(x, 3)
	_ = math.Pow(2, 2)
	_ = math.Pow(2, 3)
	_ = math.Pow(x, 6)
	_ = math.Pow(x, x)
	_ = math.Pow(x, -1)
}
