package app

import (
	"errors"

	"example.com/embedroot/lib"
)

func Check(err error) bool {
	var w lib.Wrapper
	return errors.As(err, &w)
}
