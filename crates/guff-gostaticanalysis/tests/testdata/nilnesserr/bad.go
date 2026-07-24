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
