package grouper

import (
	"fmt"
	"os"
)

const (
	a = 1
	b = 2
)

var (
	c      = 3
	d      = 4
	_sprint = fmt.Sprint
	_getenv = os.Getenv
)

type (
	e int
	f string
)
