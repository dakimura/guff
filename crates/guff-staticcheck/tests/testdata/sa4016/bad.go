package main

// Upstream has two branches (honnef `staticcheck/sa4016/sa4016.go:55-100`) and
// guff had only the second, with a condition wide enough to swallow the first's
// negative cases. The fixture that stood here was `_ = x & 0` — one operator,
// one finding, and no way to see either problem.

// Branch 1: the right operand is one of *this* package's constants written
// `name = iota`, so its value is 0 and upstream reads it as a likely mistake for
// `1 << iota`. The message says so.
const (
	flagA = iota
	flagB
	flagC
)

func iotaFlags(x int) (int, int, int) {
	return x | flagA, x & flagA, x ^ flagA
}

// Branch 2: an actual integer literal. `+0` and `-0` are literals too
// (`pattern.IntegerLiteral` allows the unary operators).
func literals(x int) (int, int, int, int) {
	return x | 0, x & 0, x ^ 0, x | +0
}

func main() {
	_, _, _ = iotaFlags(1)
	_, _, _, _ = literals(1)
}
