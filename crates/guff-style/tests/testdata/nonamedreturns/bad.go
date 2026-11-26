package nonamedreturns

func namedInt() (i int) { // want
	return 0
}

func namedErrorNoDefer() (err error) { // want
	return nil
}

func namedErrorReadOnlyDefer() (err error) { // want: read in defer but never assigned
	defer func() {
		_ = err
	}()
	return
}

func multiNamed() (a int, b string) { // want both
	return 1, "x"
}
