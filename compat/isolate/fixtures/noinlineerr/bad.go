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
