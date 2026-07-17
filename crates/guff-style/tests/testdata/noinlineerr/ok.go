package noinlineerr

func f1() error { return nil }

func compute() int { return 0 }

// Recommended form: the assignment is separate from the if.
func good() error {
	err := f1()
	if err != nil {
		return err
	}
	return nil
}

// A non-error init clause is fine.
func nonError() {
	if n := compute(); n > 0 {
		_ = n
	}
}

// The blank identifier is ignored.
func blank() {
	if _ = f1(); true {
	}
}

// The error variable is not referenced in the condition.
func notInCond() error {
	if _, err := twoResults2(); true {
		return err
	}
	return nil
}

func twoResults2() (int, error) { return 0, nil }
