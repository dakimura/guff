package main

import (
	"fmt"
	"strings"
)

// Not pure: a body that only returns a constant is a stub to irutil.IsStub,
// which upstream reads as "the real work is hidden behind a build tag".
func stub() int { return 0 }

// Not pure: no results at all.
func noResult(a int) { _ = a }

// Not pure: a parameter that is not basic (nor a struct of basics).
func first(s []int) int { return s[0] }

// Not pure: calls a function with side effects.
func logged(a int) int {
	fmt.Println(a)
	return a
}

// Not pure: panics.
func checked(a int) int {
	if a < 0 {
		panic("negative")
	}
	return a
}

func main() {
	s := strings.ToLower("x")
	_ = s
	stub()
	noResult(1)
	first([]int{1})
	logged(2)
	checked(3)
}
