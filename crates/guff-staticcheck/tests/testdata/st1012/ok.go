package pkg

import (
	"errors"
	"fmt"
)

var (
	errFoo = errors.New("")
	ErrBar = errors.New("")
	errQux = fmt.Errorf("")
)

var result = fn()

func fn() error { return nil }
