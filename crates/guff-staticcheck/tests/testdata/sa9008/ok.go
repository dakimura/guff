package main

import "fmt"

// The else reads the asserted value, not the shadowing one.
func f(x interface{}) {
	if v, ok := x.(int); ok {
		_ = v
	} else {
		fmt.Printf("%v", x)
	}
}

func g(x interface{}) {
	// !ok then-branch is the failure path; else uses the asserted value — not SA9008.
	if v, ok := x.(int); !ok {
		fmt.Printf("fail")
	} else {
		fmt.Printf("%v", v)
	}
}

// `v` shadows nothing: upstream's pattern asserts the shadowed identifier
// itself, so reading `v` in the else is left alone however zero-valued it is.
func h(x interface{}) {
	if v, ok := x.(int); ok {
		_ = v
	} else {
		fmt.Printf("%v", v)
	}
}

// `=` rather than `:=` declares nothing, so nothing is shadowed.
func i(x interface{}) {
	var y int
	var ok bool
	if y, ok = x.(int); ok {
		_ = y
	} else {
		fmt.Printf("%v", y)
	}
}
