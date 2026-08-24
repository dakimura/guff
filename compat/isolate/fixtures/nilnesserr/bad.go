package p

func do() error  { return nil }
func do2() error { return nil }

func Bad() error {
	err := do()
	if err != nil {
		return err
	}
	err2 := do2()
	if err2 != nil {
		return err // wrong error
	}
	return nil
}

// nilnesserr names the call, so a second wrong-error return reads the same but
// sits at a different position — and a variadic call is the other arm.
func AlsoBad() error {
	err := do()
	if err != nil {
		return err
	}

	err3 := do2()
	if err3 != nil {
		return err
	}

	return nil
}
