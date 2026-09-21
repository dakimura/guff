package wastedassign

import "fmt"

func wastedNeverUsed() {
	a := 0
	fmt.Print(a)
	a = 1
}

func wastedReassigned() {
	b := 0
	fmt.Print(b)
	b = 1
	b = 2
	fmt.Print(b)
}

// The recall side of the same fix: `break parsingLoop` leaves the outer loop,
// after which nothing reads `line` again — so the assignment before it is
// wasted, and upstream says so. While the labelled break resolved to the loop's
// *head* instead of its exit, the head's `line == ""` looked like a read and
// guff reported nothing here.
func labelledBreakToOuter(read func() string) int {
	n := 0
	line := read()
parsingLoop:
	for {
		if line == "" {
			return n
		}
		for {
			n++
			switch {
			case line == "end":
				line = read() + "?"
				break parsingLoop
			}
			line = read()
		}
	}
	return n
}

// The other side of the self-edge fix: this store *is* wasted, because the top
// of the next iteration overwrites `x` before reading it. Keeping the self-edge
// must not silence it — the revisit finds a Store, which is `reassignedSoon`,
// not `notWasted`.
func wastedInsideIntegerRange(n int) {
	x := 0
	for i := range n {
		x = i
		fmt.Print(x)
		x = 99
	}
}

// The `if`-init store whose value the condition reads is excused because
// NaiveForm does not Load it. Keyed on the *object* rather than on the store,
// that excuse also covered the declaration above the loop, which has nothing
// to do with the `if`.
func ifInitCondRead(tree map[string]map[string]int, components []string) int {
	dir, exists := tree, false
	for _, item := range components {
		var sub map[string]int
		if sub, exists = dir[item]; !exists {
			return 0
		}
		_ = sub
	}
	return 1
}

func trim(s string) string { return s }

// An assignment whose right-hand side mentions the variable: the operands are
// evaluated *before* the store, so that mention is not a later read.
func selfReferencingParam(name string) string {
	name = trim(name)
	return "x"
}

func selfReferencingThenReassigned() string {
	s := "q"
	s = trim(s)
	s = "z"
	return s
}

// The control: here the value really is read, so the store is live.
func selfReferenceIsARealRead() string {
	s := "q"
	s = trim(s)
	return s
}

// Upstream's `srcFuncs` is every named function in the package's AST, methods
// included. A members-only list leaves out every method in the package.
type tree map[string]tree

func (t tree) methodWastedStore() int {
	x := 1
	x = 2
	return x
}

func (t *tree) pointerMethodWastedStore() int {
	x := 1
	x = 2
	return x
}

// A method, an `if`-init whose condition reads the variable, and a declaration
// above the loop that has nothing to do with it.
func (t tree) getByComponents(components []string) (tree, error) {
	dir, exists := t, false
	for _, item := range components {
		if len(item) != 0 {
			if dir == nil {
				return nil, nil
			}
			if dir, exists = dir[item]; !exists {
				return nil, nil
			}
		}
	}
	return dir, nil
}

// `compLit` writes an array or struct literal *into the address*, so no `Store`
// exists and there is nothing to report. A slice or map literal is built as a
// value and then stored, and that `Store` carries the literal's `Lbrace` — not
// the assignment's `=`. Reporting every one of them at the `=` put a finding on
// beats' `x-pack/metricbeat/module/aws/billing/billing.go:209` (`event :=
// mb.Event{}`) that upstream does not make.
type elem struct{ N int }

func mkElem() elem          { return elem{} }
func mkSlice() []int        { return nil }
func mkMap() map[string]int { return nil }
func mkArray() [3]int       { return [3]int{} }

// Silent: a struct literal.
func structLiteralInit(c bool) elem {
	e := elem{}
	if c {
		e = mkElem()
	} else {
		e = mkElem()
	}
	return e
}

// Silent: an array literal, elements and all.
func arrayLiteralInit(c bool) [3]int {
	a := [3]int{1, 2, 3}
	if c {
		a = mkArray()
	} else {
		a = mkArray()
	}
	return a
}

// Reported, at the `{`.
func sliceLiteralInit(c bool) []int {
	s := []int{1}
	if c {
		s = mkSlice()
	} else {
		s = mkSlice()
	}
	return s
}

func mapLiteralInit(c bool) map[string]int {
	m := map[string]int{"a": 1}
	if c {
		m = mkMap()
	} else {
		m = mkMap()
	}
	return m
}

// `&elem{}` is a UnaryExpr around the literal, not a literal: an ordinary
// store at the `=`.
func pointerToLiteralInit(c bool) *elem {
	p := &elem{}
	if c {
		p = nil
	} else {
		p = nil
	}
	return p
}
