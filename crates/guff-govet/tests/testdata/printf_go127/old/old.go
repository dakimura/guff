//go:build go1.26

// A //go:build go1.26 line lowers this file's version below go1.27 in a
// go 1.27 module: neither rule applies here.
package old

import "fmt"

type CustomError struct{ Msg string }

func (e CustomError) Error() string { return e.Msg }

func F(i int) {
	_ = fmt.Errorf("%w", &CustomError{})
	_ = fmt.Sprintf("%d", &i)
}
