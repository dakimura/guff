package nonamedreturnsok

func unnamed() (int, error) {
	return 0, nil
}

func underscore() (_ int, _ error) {
	return 0, nil
}

func errorInDeferAssigned() (err error) {
	defer func() {
		err = nil
	}()
	return
}

func errorReadInDeferAssignedInBody() (err error) {
	defer func() {
		_ = err
	}()
	err = nil
	return
}

func errorReadInDeferAssignedViaReturn() (err error) {
	defer func() {
		if err != nil {
			_ = err
		}
	}()
	return nil
}
