package complitpartial

// SA4006 and array literals assigned to a variable, measured against
// golangci-lint 2.15.0 (staticcheck v0.8.2) one function per shape.
//
// staticcheck's IR initializes a composite literal in place when the
// location is a plain address. A partial array literal (some elements, not
// all) is then written through IndexAddrs of the variable itself, so the
// variable stays in memory and the literal's value is never "unused": none of
// the partial shapes below is reported, however dead the value is. An array
// literal that lists every element, an empty one, or a slice literal is built
// as one value and stored once, so overwriting it unread is reported.

func g() int { return 1 }

type A [3]int

type S struct{ arr [3]int }

func existingPartial() {
	var ar [3]int
	_ = ar
	ar = [3]int{g()} // not reported: partial, initialized in place
	ar = [3]int{}
	_ = ar
}

func definePartial() {
	x := [3]int{g()} // not reported
	x = [3]int{}
	_ = x
}

func keyedPartial() {
	var ar [3]int
	_ = ar
	ar = [...]int{2: g()} // not reported: [3]int with one element
	ar = [3]int{}
	_ = ar
}

func keyedFull() {
	var ar [3]int
	_ = ar
	ar = [3]int{2: g(), 0: 1, 1: 2} // reported: every element, keyed
	ar = [3]int{}
	_ = ar
}

func fullArray() {
	var ar [3]int
	_ = ar
	ar = [3]int{1, 2, g()} // reported
	ar = [3]int{}
	_ = ar
}

func emptyArray() {
	ar := [3]int{1, 2, 3}
	_ = ar
	ar = [3]int{} // not reported: the zero constant
	ar = [3]int{1, 2, 3}
	_ = ar
}

func interfaceElems() {
	var ar [3]any
	_ = ar
	ar = [3]any{g()} // not reported
	ar = [3]any{}
	_ = ar
}

func namedArray() {
	var a A
	_ = a
	a = A{g()} // not reported
	a = A{}
	_ = a
}

func intoInterface() {
	var w any
	_ = w
	w = [3]int{g()} // not reported: no in-place for an interface; MakeInterface
	w = nil
	_ = w
}

func parallel() {
	var a, b [3]int
	_, _ = a, b
	a, b = [3]int{g()}, a // not reported
	a = [3]int{}
	_, _ = a, b
}

func capturedByClosure() func() [3]int {
	var ar [3]int
	f := func() [3]int { return ar }
	ar = [3]int{g()} // not reported: the closure reads it
	return f
}

func readAfter() [3]int {
	var ar [3]int
	_ = ar
	ar = [3]int{g()} // not reported: returned
	return ar
}

func readBeforeOverwrite() int {
	ar := [3]int{g()} // not reported: ar[0] reads it
	n := ar[0]
	ar = [3]int{}
	_ = ar
	return n
}

func bigKeyed() {
	var ar [100]int
	_ = ar
	ar = [100]int{5: g()} // not reported
	ar = [100]int{}
	_ = ar
}

func sliceOfArrays() {
	s := [][3]int{{g()}} // reported: the slice value is unused
	s = nil
	_ = s
}

func fullOuterPartialInner() {
	var ar [2][3]int
	_ = ar
	ar = [2][3]int{{g()}, {1, 2, 3}} // reported: the outer literal is full
	ar = [2][3]int{}
	_ = ar
}

func partialOuter() {
	var ar [2][3]int
	_ = ar
	ar = [2][3]int{{g()}} // not reported
	ar = [2][3]int{}
	_ = ar
}

func structWithArrayField() {
	var s S
	_ = s
	s = S{arr: [3]int{g()}} // reported: a struct literal is one value
	s = S{}
	_ = s
}

func parenthesized() {
	var ar [3]int
	_ = ar
	ar = ([3]int{g()}) // not reported
	ar = [3]int{}
	_ = ar
}

func gappedSlice() {
	s := []int{5: g()} // reported: a slice literal is a fresh value
	s = nil
	_ = s
}

var global [3]int

func intoGlobal() {
	global = [3]int{g()} // never reported: not a local
	global = [3]int{}
}

func throughPointer(p *[3]int) {
	*p = [3]int{g()} // never reported: not an identifier
	*p = [3]int{}
}

func intoField(s *S) {
	s.arr = [3]int{g()} // never reported: not an identifier
	s.arr = [3]int{}
}
