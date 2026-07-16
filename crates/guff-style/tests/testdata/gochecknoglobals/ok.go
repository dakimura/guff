package gochecknoglobalsok

import (
	"errors"
	"regexp"
)

const constant = 0

var _ = 0

var version string

var errUnexported = errors.New("errUnexported")

var ErrExported = errors.New("ErrExported")

var errCustom = &customError{"x"}

var ErrCustom = &customError{"y"}

var errValueRecv = customError2{"z"}

var IsOnlyDigitsRe = regexp.MustCompile(`^\d+$`)

var (
	PrecompileOne = regexp.MustCompile(`[a-z]{1,3}`)
	PrecompileTwo = regexp.MustCompile(`[a-z]{3,6}`)
)

//go:embed ignored.txt
var embedded string

var (
	//go:embed ignored.txt
	groupedEmbed string
)

type customError struct{ e string }

func (e *customError) Error() string { return e.e }

type customError2 struct{ e string }

func (e customError2) Error() string { return e.e }

func localOk() {
	x := 1
	_ = x
}
