package p

func foo() (err error) {
	if true {
		return
	}
	if false {
		return func() (err2 error) {
			return
		}
	}
	return
}

func bar() (_ int, err error) {
	return
}

func baz() (a, b, c int) {
	return
}

func qux() (file string, b int, err error) {
	if err == nil {
		return
	}

	// A comment
	return
}

// quux does quuxy things
func quux() {}
