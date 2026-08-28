package p

func do() error { return nil }

func Bad() {
	if err := do(); err != nil {
		panic(err)
	}
}

// Only the `if` form is reported. `switch err := do(); err` and
// `for err := do(); …` carry the same inline assignment and upstream says
// nothing about either, so these two are negatives — a future widening of the
// rule would show up here as an extra.
func SwitchInline() {
	switch err := do(); err {
	case nil:
	default:
	}
}

func ForInline() {
	for err := do(); err != nil; {
		break
	}
}

func two() (error, error) { return nil, nil }

// Two error names on the left. Upstream's guard trips on `len(Lhs) != 1` and
// then *returns*, so only the first is reported — the second never gets its own
// finding. Without the return this fixture would show two.
func MultiLHS() {
	if err1, err2 := two(); err1 != nil || err2 != nil {
		panic(err1)
	}
}

// `err` already lives in the scope the hoisted assignment would land in, so
// `err := do()` there would be a redeclaration. Upstream withholds the fix.
func ShadowedInParentScope() {
	err := do()
	_ = err

	if err := do(); err != nil {
		panic(err)
	}
}
