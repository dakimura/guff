package pkg

import "math"

func g() float64 { return 1 }

func fn() {
	var x float64
	var y int

	_ = 1.0
	_ = x
	_ = x * x
	_ = x * x * x

	// Exponents upstream will not expand.
	_ = math.Pow(x, 6)
	_ = math.Pow(x, x)
	_ = math.Pow(x, -1)

	// A base that may have side effects is refused *before* the exponent is
	// read, so none of these six is reported — not even the ones whose
	// replacement would not mention the base at all. Until 2026-08-27 guff
	// gated on `n >= 2` instead, which reported all six and rewrote
	// `math.Pow(g(), 0)` to `1.0`, deleting the call (COMPAT-HARDENING 続き 73).
	_ = math.Pow(g(), 0)
	_ = math.Pow(g(), 1)
	_ = math.Pow(g(), 2)
	_ = math.Pow(float64(y), 0)
	_ = math.Pow(float64(y), 1)
	_ = math.Pow(float64(y), 2)
}
