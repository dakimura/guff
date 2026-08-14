package main

// Upstream walks `*ast.IfStmt` only, and proves the operand never-nil by
// walking the IR back to a MakeChan / MakeMap / MakeSlice / Alloc / Function /
// MakeClosure. A bare comparison outside an `if` is not a finding — see ok.go.
//
// Verified against golangci-lint 2.12.2 (with issues.max-same-issues disabled;
// the default of 3 hides the fifth).
func main() {
	c := make(chan int)
	if c == nil {
		return
	}
	p := new(int)
	if p == nil {
		return
	}
	s := []int{}
	if s == nil {
		return
	}
	f := main
	if f == nil {
		return
	}
	var x int
	q := &x
	if q == nil {
		return
	}
}
