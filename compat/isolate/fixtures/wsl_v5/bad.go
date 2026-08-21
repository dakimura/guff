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

// The two shapes below are also silent upstream, and were guff-only false
// positives in authelia. `checkError` has two early returns guff did not.

// `previousIdents` comes from an `*ast.AssignStmt`'s LHS or an `*ast.DeclStmt`'s
// names, and from nothing else: an `if` above whose *init* assigns err
// contributes no idents, so the intersection is empty and upstream returns.
func ErrAssignedInIfInit(v string) error {
	var err error

	if _, err = parse(v); err == nil {
		_ = v
	}

	if err != nil {
		return err
	}

	return nil
}

// A comment between the assignment and the check is content. Upstream only
// removes the blank line when the comment ends on the assignment's own line.
func ErrWithCommentBetween(v string) error {
	_, err := parse(v)

	// Deliberate: the reason for checking is worth a line of its own.
	if err != nil {
		return err
	}

	return nil
}

// The positive control for both: no comment, plain assignment above, so the
// blank line *is* reported. Silencing everything does not pass this fixture.
func ErrWithBlankLine(v string) error {
	_, err := parse(v)

	if err != nil {
		return err
	}

	return nil
}

func parse(string) (int, error) { return 0, nil }
