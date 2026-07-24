package nilerr

func do() error { return nil }

func badNotNil() error {
	err := do()
	if err != nil {
		return nil
	}
	return nil
}

func badIsNil() error {
	err := do()
	if err == nil {
		return err
	}
	return err
}
