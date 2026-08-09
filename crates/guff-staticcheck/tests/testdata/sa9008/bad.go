package main

import "fmt"

// The else branch reads the shadowing `x`, which holds the zero value of int.
func f(x interface{}) {
	if x, ok := x.(int); ok {
		_ = x
	} else {
		fmt.Printf("%v", x)
	}
}

// Same, with a struct type and the read nested in a call argument.
type httpError struct{ code int }

func (httpError) Error() string { return "http" }

func g(err error) string {
	if err, ok := err.(httpError); ok {
		return fmt.Sprintf("%d", err.code)
	} else {
		return fmt.Sprintf("%v", err)
	}
}
