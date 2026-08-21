package wsl_v5

func Ok() {
	b := 2
	if b == 2 {
		return
	}

	used := true
	if used {
		return
	}

	var a = 1

	var b2 = 2

	_ = a
	_ = b2

	err := doErr()
	if err != nil {
		return
	}
}

func doErr() error { return nil }

func Short() int {
	x := 1
	return x
}

// `checkError` builds its `previousIdents` from an `*ast.AssignStmt`'s LHS or an
// `*ast.DeclStmt`'s names and from nothing else, so an `if` above whose *init*
// assigns err contributes no idents and upstream returns before reporting.
func ErrAssignedInIfInit(v string) error {
	var err error

	if _, err = parseIt(v); err == nil {
		_ = v
	}

	if err != nil {
		return err
	}

	return nil
}

// A comment on a line of its own between the assignment and the check is
// content: upstream only removes the blank line when the comment ends on the
// assignment's own last line.
func ErrWithCommentBetween(v string) error {
	_, err := parseIt(v)

	// Worth a line of its own.
	if err != nil {
		return err
	}

	return nil
}

func parseIt(string) (int, error) { return 0, nil }
