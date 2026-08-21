package p

func Bad() {
	one := 1
	two := 2
	three := 3
	if three == 3 {
		_ = one
		_ = two
		return
	}
	four := 4
	_ = four
}

// The three shapes below are *silent* upstream, and each was a guff-only false
// positive found by adding authelia to the corpus (176 of them in one repo).

// An assignment may cuddle an expression statement when the two share an
// identifier: the `assign-expr` check that forbids it is not on by default.
func AssignCuddledWithSharingExpr(c *cfg) {
	report(c.value)
	c.value = 2
}

// The `expr` check does not enforce cuddle-max-statements — upstream passes
// enforceLimit=false for expression statements.
func ExprAfterManyAssigns(c *cfg) {
	a := 1
	b := 2
	report(a + b + c.value)
}

// A comment between `{` and the first statement is content, not a blank line.
func CommentFirstInBlock(c *cfg) {
	//nolint:gosec
	var x int

	c.value = x
}

type cfg struct{ value int }

func report(int) {}
