package errorlint

type sentinel string

func (s sentinel) Error() string { return string(s) }

var errFoo error = sentinel("foo")

func bad(err error) {
	if err == errFoo {
		_ = err
	}
}
