package noinlineerr

func doSomething() error {
	return nil
}

func twoResults() (int, error) {
	return 0, nil
}

type myError struct{}

func (myError) Error() string { return "my" }

func newMyErr() *myError {
	return &myError{}
}

// Inline error handling: `err` is an error used in the condition.
func a() error {
	if err := doSomething(); err != nil {
		return err
	}
	return nil
}

// Multi-assign: only `err` is flagged (`n` is an int and unused in the cond).
func b() error {
	if n, err := twoResults(); err != nil {
		_ = n
		return err
	}
	return nil
}

// Concrete type implementing error is also assignable to `error`.
func c() error {
	if e := newMyErr(); e != nil {
		return e
	}
	return nil
}
