package complit

// SA4006 around composite literals, one shape per branch of the SSA
// builder's compLit: struct (keyed, positional, nested), array (full), slice,
// map, `&T{...}`, a slice of struct literals, a map whose value is a slice
// literal, and a store into an interface. Each literal element slot stores
// through an address that records its value expression for DebugRef, and a
// slice or map literal's final store records the literal itself; these are
// the shapes whose first DebugRef for an expression could move.
//
// Not here: a partial array literal assigned to a variable (`ar =
// [3]int{g()}` then overwritten), which staticcheck's IR initializes in place
// and never reports — those shapes are in complit_partial.go.

type T struct {
	A int
	B any
}

func g() int { return 1 }

func structKeyedReadsVar() {
	x := g()
	_ = T{A: x}
	x = g() // reported: the second value of x is never read
}

func sliceOverwritten() []int {
	s := []int{g()} // reported
	s = nil
	return s
}

func mapOverwritten() map[int]any {
	m := map[int]any{1: g()} // not reported upstream
	m = nil
	return m
}

func structInterfaceField() {
	v := T{B: g()} // reported
	v = T{}
	_ = v
}

func arrayFull() {
	a := [2]any{g(), g()} // reported
	a = [2]any{}
	_ = a
}

func sliceInterfaceElems() {
	s := []any{g()}
	_ = s
	s = []any{1} // reported
}

func addrOfStruct() *T {
	p := &T{A: g()} // not reported upstream
	p = nil
	return p
}

func sliceOfStructs() {
	ts := []T{{A: g()}} // reported
	ts = nil
	_ = ts
}

func interfaceVar() {
	var w any
	w = g() // not reported upstream: the stored value is a MakeInterface
	w = 2
	_ = w
}

func mapOfSlices() {
	m := map[string][]int{"a": {g()}} // not reported upstream
	m = nil
	_ = m
}

func positionalStruct() {
	v := T{g(), g()} // reported
	v = T{A: 1}
	_ = v
}

func nestedPointerSlice() {
	n := []*T{{A: g()}} // reported
	n = nil
	_ = n
}
