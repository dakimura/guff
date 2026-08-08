package main

type MySlice []string

func usedAfterInc() {
	x := 1
	_ = x
	x = 2
	_ = x
}

func loopClassic(n int) int {
	sum := 0
	for i := 0; i < n; i++ {
		sum += i
	}
	return sum
}

func loopBodyInc(n int) int {
	x := 0
	for x < n {
		x++
	}
	return x
}

func usedInc() {
	var n int
	n++
	println(n)
}

func usedAdd() {
	n := 1
	n += 1
	println(n)
}

// `n++` is an *ast.IncDecStmt; upstream only walks *ast.AssignStmt, so an
// increment whose result nothing reads is still not a finding.
func unusedInc() {
	var n int
	n++
}

// `n += 1` is an AssignStmt, but it is judged by its right-hand side — the
// constant `1` — and constants are skipped.
func unusedAdd() {
	n := 1
	n += 1
}

// Same reason: the right-hand side is the constant `1`.
func overwrittenByConst() {
	var n int
	n = 1
	n = 2
	_ = n
}

// A conversion that only re-labels an existing value (a ChangeType in IR) is
// not reported, unlike a real conversion such as `string(b)`.
func relabelConversion(y []string) {
	x := []string{"a"}
	_ = x
	x = MySlice(y)
}

// Boxing into an interface (a MakeInterface in IR) is skipped for the same
// reason.
func interfaceBoxing(n int) {
	var i interface{} = 1
	_ = i
	i = n
}

type chainable struct{ n int }

func (c chainable) skip(s string) chainable { return chainable{c.n + len(s)} }

// Go evaluates the right-hand side before the assignment takes effect, so a
// value the overwriting statement itself reads is not dead — even though the
// target ident sits to the left of that read. consul's
// `internal/protohcl/unmarshal_test.go` and grafana's `evaluator_test.go` were
// false positives until this was recognised.
func chainedOverwrite() chainable {
	c := chainable{1}
	c = c.skip("a")
	c = c.skip("b")
	return c
}

// Same, via `:=` where only the second name is newly declared, so `c` is an
// assignment target rather than a definition.
func chainedShortDecl() (chainable, int) {
	c := chainable{1}
	c, extra := c.skip("a"), 2
	return c, extra
}
