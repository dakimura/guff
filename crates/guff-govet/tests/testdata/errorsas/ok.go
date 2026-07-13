package ok

import "errors"

type stringer interface {
	String() string
}

func ok() {
	var err error
	var target stringer
	errors.As(err, &target)
}
