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
