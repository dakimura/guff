package scope

import "fmt"

type Key string

func K(s string) Key { return Key(s) }

// Three in production code: a finding of 3, not 5 (the test file's two
// are counted in their own scope).
func prod() string {
	a := "shared-value"
	b := "shared-value"
	if a == b {
		return "shared-value"
	}
	return ""
}

// Only the test file declares a constant for this value: the production
// finding must not name it.
func prodOnly() {
	x := "test-only-const"
	y := "test-only-const"
	z := "test-only-const"
	_, _, _ = x, y, z
}

// Map keys: plain string keys and K("...") keys are dropped under
// ignore-map-keys; the values still count.
var m1 = map[string]string{"mapkey": "mapvalue"}
var m2 = map[string]string{"mapkey": "mapvalue"}
var m3 = map[string]string{"mapkey": "mapvalue"}
var m4 = map[Key]int{K("exprkey"): 1}
var m5 = map[Key]int{K("exprkey"): 2}
var m6 = map[Key]int{K("exprkey"): 3}

// ignore-functions: fmt.Println's arguments are not counted, Sprint's are.
func calls() {
	fmt.Println("ignored-call")
	fmt.Println("ignored-call")
	fmt.Println("ignored-call")
	_ = fmt.Sprint("counted-call")
	_ = fmt.Sprint("counted-call")
	_ = fmt.Sprint("counted-call")
}

// The smallest position in the file is reported, not the first visited:
// the composite literal's value is walked before the nested literal's.
type Obj struct{ Name string }
type Wrap struct {
	O Obj
	E string
}

func order() {
	w := Wrap{O: Obj{Name: "order-value"}, E: "order-value"}
	w2 := "order-value"
	_, _ = w, w2
}
