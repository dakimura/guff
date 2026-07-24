package nilnesserr_ok

func do() error {
	return nil
}

func do2() error {
	return nil
}

func do3() (int, error) {
	return 0, nil
}

func okSameErr() error {
	err := do()
	if err != nil {
		return err
	}
	err2 := do2()
	if err2 != nil {
		return err2
	}
	return err
}

func okShadow() error {
	err := do()
	if err != nil {
		return err
	}
	if err := do2(); err != nil {
		return err
	}
	return err
}

func okReassign() (int, error) {
	res, err := do3()
	if err != nil {
		return 0, err
	}
	return res, err
}

func okNilCheckReturnNil() error {
	err := do()
	if err == nil {
		return err
	}
	return err
}
