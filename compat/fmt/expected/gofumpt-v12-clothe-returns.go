package p

func foo() (err error) {
	if true {
		return err
	}
	if false {
		return func() (err2 error) {
			return err2
		}
	}
	return err
}

func bar() (_ int, err error) {
	return
}

func baz() (a, b, c int) {
	return a, b, c
}

func qux() (file string, b int, err error) {
	if err == nil {
		return file, b, err
	}

	// A comment
	return file, b, err
}

// quux does quuxy things
func quux() {}
