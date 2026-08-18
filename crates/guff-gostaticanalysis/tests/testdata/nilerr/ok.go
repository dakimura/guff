package nilerr_ok

func do() error { return nil }

func wrap(err error) error { return err }

func ok() error {
	err := do()
	if err != nil {
		return err
	}
	return nil
}

func okNilBranch() error {
	err := do()
	if err == nil {
		return nil
	}
	return err
}

func okUseErr() error {
	err := do()
	if err != nil {
		return wrap(err)
	}
	return nil
}

func logf(format string, args ...any) {}

// Passing the error to anything variadic boxes it into an `any` first, so the
// use is a `MakeInterface` wrapping it — `isUsedInValue` peels that. Without
// the peel the block looks as though it never mentions the error, which is
// what made dapr's 25 `fmt.Sprintf("…: %v", err)` blocks findings.
func okErrBoxedIntoAny() error {
	err := do()
	if err != nil {
		logf("failed: %v", err)
		return nil
	}
	return nil
}
