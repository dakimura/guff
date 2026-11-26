package pkg

import "math"

const untypedInt = 2
const untypedFloat = 2.0
const untypedRune = 'a'
const typedFloat float64 = 2

func fn() {
	var x float64

	// The shapes this fixture held until 2026-08-27: a non-constant base and
	// one untyped-int literal. Everything below them was unmeasured, and six of
	// QF1005's seven defects were hiding there. The seventh is in ok.go, which
	// held no base that may have side effects (COMPAT-HARDENING 続き 73).
	_ = math.Pow(x, 0)
	_ = math.Pow(x, 1)
	_ = math.Pow(x, 2)
	_ = math.Pow(x, 3)
	_ = math.Pow(2, 2)
	_ = math.Pow(2, 3)

	// --- does the base need a float64 conversion? ---------------------------
	//
	// Upstream re-type-checks the base *on its own* and wraps unless it would
	// be untyped-float or float64 there. The argument's recorded type is
	// float64 for every one of these, so a predicate that reads it says "no
	// conversion" every time.

	// Untyped int, in all the spellings that reach the same answer.
	_ = math.Pow(untypedInt, 2)
	_ = math.Pow(1+1, 2)
	_ = math.Pow(4/2, 2)
	// A shift takes the kind of its *left* operand, so this is untyped int.
	_ = math.Pow(1<<3, 2)
	_ = math.Pow(-2, 2)
	_ = math.Pow((2), 2)

	// Untyped rune is an integer kind too, so it is wrapped.
	_ = math.Pow(untypedRune, 2)
	_ = math.Pow('a', 2)

	// Untyped float and typed float64 are not.
	_ = math.Pow(untypedFloat, 2)
	_ = math.Pow(2.0, 2)
	_ = math.Pow(-2.5, 2)
	_ = math.Pow(5.0/2, 2)
	_ = math.Pow(math.Pi, 2)
	_ = math.Pow(typedFloat, 2)

	// A mixed operation takes the wider of the two kinds, so one untyped-float
	// operand is enough to keep the whole expression out of the wrap.
	_ = math.Pow(2.0*2, 2)

	// The conversion rides on n == 1 as well, and never on n == 0 — `1.0` is
	// already an untyped float.
	_ = math.Pow(2, 1)
	_ = math.Pow(2, 0)

	// --- how the product is printed ----------------------------------------
	//
	// Upstream builds a left-associative BinaryExpr, drops redundant parens,
	// and lets go/printer decide the rest: the left operand keeps its parens
	// only below `*`'s precedence, the right operand keeps them at or below.

	// Same precedence as `*`: left bare, right parenthesized.
	_ = math.Pow(x/2, 2)
	// Lower precedence: both parenthesized.
	_ = math.Pow(x+1, 2)
	// Unary binds tighter than `*`: no parens at all.
	_ = math.Pow(-x, 2)
	// Three factors, so the chain nests on the left.
	_ = math.Pow(2+3, 3)
	_ = math.Pow(x+1, 3)

	// A base that is itself a `*` is the one shape where no parenthesis
	// survives on either side, at any n. `SimplifyParentheses` rotates
	// `a * (b * c)` into `(a * b) * c` when the operators match, and repeats,
	// so the whole product flattens. A base at the same precedence but a
	// different operator does not rotate and keeps its parentheses.
	_ = math.Pow(x*2, 2)
	_ = math.Pow(x*2, 3)
	_ = math.Pow(2.0*2, 3)
	_ = math.Pow(x/2, 3)
	_ = math.Pow(x*2+1, 2)
}
