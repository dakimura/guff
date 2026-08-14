package main

import "math"

// Upstream asks for an `ir.Convert` whose operand is an integer, so what it
// reports is a *conversion*, never a literal: `math.Ceil(1)` has no conversion
// at all — the constant is already float64 — and lives in ok.go.
//
// Verified against golangci-lint 2.12.2 alongside the shapes in ok.go.
func main() {
	var n int
	var u uint8
	var i64 int64
	_ = math.Ceil(float64(n))
	_ = math.Floor(float64(u))
	_ = math.IsNaN(float64(n))
	_ = math.Ceil(float64(i64))
}
