package minmax

// Upstream's minmax has two patterns and guff had only the first. Pattern 2 is
// `lhs0 = rhs0` immediately above `if a < b { lhs = rhs }` with no else, and it
// says "if statement", where pattern 1 says "if/else statement".
func clampMin(x, y int) int {
	v := x
	if v > y {
		v = y
	}

	return v
}

func clampMax(x, y int) int {
	v := x
	if v < y {
		v = y
	}

	return v
}

// `=` rather than `:=`: the fix has to reuse the token it found.
func clampPlainAssign(x, y int) int {
	var v int

	v = x
	if v > y {
		v = y
	}

	return v
}

// Silent: the statement above assigns a different variable.
func otherVarAbove(x, y int) (int, int) {
	v := y
	w := x
	if v > y {
		v = y
	}

	return w, v
}

// Silent: the operands do not come from the assignment above.
func unrelatedOperands(x, y, z int) int {
	v := x
	if y > z {
		v = z
	}

	return v
}

// Silent: floats may be NaN.
func clampFloat(x, y float64) float64 {
	v := x
	if v > y {
		v = y
	}

	return v
}

// Pattern 1 still owns the if/else shape, and still says "if/else statement".
func withElse(x, y int) int {
	var v int

	if x > y {
		v = y
	} else {
		v = x
	}

	return v
}

// The matching is `astutil.EqualSyntax` — the written shape, identifiers by
// name — and not "the same value", so `len(a)` matches `len(a)` although a call
// is not provably pure. syncthing writes this five times.
func clampToLen(a, b []string) int {
	count := len(a)
	if len(b) < len(a) {
		count = len(b)
	}

	return count
}

// And the two spellings of one expression differ only in spacing, which is not
// part of the syntax.
func clampArith(buf []byte, written, chunk int) int {
	toWrite := chunk
	if toWrite > len(buf)-written {
		toWrite = len(buf) - written
	}

	return toWrite
}
