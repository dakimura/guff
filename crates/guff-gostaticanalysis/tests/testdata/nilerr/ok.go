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
