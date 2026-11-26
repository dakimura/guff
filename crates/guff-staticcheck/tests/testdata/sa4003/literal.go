package main

// honnef's `IntegerLiteral` is a *syntactic* pattern —
// `(Or (BasicLit "INT" _) (UnaryExpr (Or "+" "-") (IntegerLiteral _)))` — and
// SA4003 says why in a comment: "We only check for the math constants and
// integer literals, not for all constant expressions. This is to avoid false
// positives when constant values differ under different build tags."
//
// guff answered it with the folded constant, so any named constant that
// happens to hold the right number matched. velero's
// `entry.Logger.GetLevel() >= logrus.PanicLevel` is that shape.

type Level uint32

const (
	PanicLevel Level = iota
	FatalLevel
)

const zero = 0

const one = 1

// Reported: a literal, in each of its spellings, and through a sign or a
// parenthesis (upstream's matcher strips parentheses before it dispatches).
func literals(l Level) {
	_ = l >= 0
	_ = l >= 0x0
	_ = l >= 0o0
	_ = l >= +0
	_ = l >= (0)
}

// Silent: a named constant is not a literal, however it was declared and
// whatever it holds.
func constants(l Level) {
	_ = l >= PanicLevel
	_ = l >= zero
}

// Silent: nor is a folded expression.
func folded(l Level) {
	_ = l >= 1-1
}

// SA4024 asks the same question of `len(x) < 0`.
func lengths(s string) {
	_ = len(s) < 0
	_ = len(s) < zero
}

// And SA4028 of `x % 1`.
func modulo(n int) {
	_ = n % 1
	_ = n % one
}
