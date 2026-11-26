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

// Reported since golangci-lint 2.14.0 (staticcheck v0.8.1), which walks
// *ast.IncDecStmt too; 2.12.2 only walked *ast.AssignStmt and said nothing.
func unusedInc() {
	var n int
	n++
}

// Reported since 2.14.0: the right-hand constant has no IR value, so a `+=`
// is judged by the left-hand side's — the sum, which nothing reads.
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

func mu()                 {}
func fs() (string, error) { return "", nil }

// A deferred call can assign to a named result, so go/ssa keeps those cells
// addressable in a function that defers — the overwritten value is reachable
// through the cell and is not dead. Lifting them anyway reported rclone's
// `cmd/serve/rc.go` `startRc`, whose `err` is a named result.
func namedResultUnderDefer() (out int, err error) {
	s, err := fs()
	defer mu()
	if s == "" {
		return 0, err
	}
	err = nil
	return 1, err
}

// A loop's post statement runs on `continue` too: go/ssa (and honnef IR) give
// it a `for.post` block that `continue` jumps to, so `i++` reaches the loop
// header and its value is read by the condition. guff used to send `continue`
// straight to the header, which left `i++` unreachable in a loop whose body
// otherwise leaves — and SA4006 called it unused (gin `gin.go:709`).
func postAfterContinueThenBreak(t []int, m int) int {
	for i, tl := 0, len(t); i < tl; i++ {
		if t[i] != m {
			continue
		}
		break
	}
	return 0
}

func postAfterContinueThenReturn(t []int, m int) int {
	for i := 0; i < len(t); i++ {
		if t[i] != m {
			continue
		}
		return 1
	}
	return 0
}

// An unconditional `break`: the post statement is unreachable, its block is
// deleted, and there is no value to judge.
func postNeverRuns(t []int) int {
	i := 0
	for ; i < len(t); i++ {
		break
	}
	return i
}
