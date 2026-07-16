package pkg

import (
	"errors"
	"fmt"
)

var (
	foo    = errors.New("")
	errBar = errors.New("")
	abc    = fmt.Errorf("")
	errAbc = fmt.Errorf("")
)

var wrong = errors.New("")

var result = fn()

func fn() error { return nil }
