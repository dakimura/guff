package nilnesserr

func do() error {
	return nil
}

func do2() error {
	return nil
}

func wrap(err error) error {
	return err
}

func join(errs ...error) error {
	if len(errs) == 0 {
		return nil
	}
	return errs[0]
}

func badReturn() error {
	err := do()
	if err != nil {
		return err
	}
	err2 := do2()
	if err2 != nil {
		return err
	}
	return nil
}

func badCall() error {
	err := do()
	if err != nil {
		return err
	}
	err2 := do2()
	if err2 != nil {
		return wrap(err)
	}
	return nil
}

func badVariadic() error {
	err := do()
	if err != nil {
		return err
	}
	err2 := do2()
	if err2 != nil {
		return join(err)
	}
	return nil
}

func badMulti() (error, error) {
	err := do()
	if err != nil {
		return nil, err
	}
	err2 := do2()
	if err2 != nil {
		return err, err2
	}
	return nil, nil
}

// The variadic element type decides whether the value is widened. `join(errs
// ...error)` above takes the error as it is; `logf(format string, args ...any)`
// wraps it in a `ChangeInterface`, and a check that does not read through that
// sees an `any`, decides it is not an error, and says nothing.
func logf(format string, args ...any) {}

func badWidened() {
	err := do()
	if err != nil {
		return
	}

	if err := do2(); err == nil {
		return
	}

	logf("failed: %v", err)
}

// silent — the error the message names is the one that was checked.
func okWidened() {
	err := do()
	if err != nil {
		return
	}

	logf("failed: %v", err)
}
