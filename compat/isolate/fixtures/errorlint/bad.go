package p

import "errors"

var errSentinel = errors.New("x")

func Bad(err error) bool {
	return err == errSentinel
}
