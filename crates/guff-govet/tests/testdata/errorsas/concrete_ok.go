package ok

import "errors"

type myError struct{}

func (e *myError) Error() string { return "my" }

func ok() {
	var err error
	var target *myError
	errors.As(err, &target)
}
