package p

import "fmt"

func Bad(cond bool) int {
	n := 1
	if cond {
		n = 2
	}
	n = 3
	return n
}

// A labelled `break` leaves its loop, so the outer loop's condition reads
// `line` again and the assignment before the break is live. Resolving that
// break to the loop's *head* instead — which guff did while the label's break
// target was recorded only after the body was built — turns this into a
// finding neither tool should have.
func LiveAcrossLabelledBreak(read func() string) int {
	n := 0
	line := read()
	for {
		if line == "" {
			return n
		}
	curFileLoop:
		for {
			line = read()
			n++
			switch {
			case line == "end":
				line = read() + "!"
				break curFileLoop
			}
		}
	}
}

// The other direction: breaking the *outer* loop reaches the return, so nothing
// reads `line` again and the assignment is wasted.
func WastedAcrossLabelledBreak(read func() string) int {
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

// A local `const` declares no storage. go/ssa's `case *ast.DeclStmt` builds a
// cell only when `d.Tok == token.VAR`; guff built one for every ValueSpec, so
// the Store for an unread constant looked exactly like a wasted assignment.
// gitea hit it three times — migrations declare a nine-name `const (…)` block
// inside the migration function and read only some of the names.
//
// Five const shapes, all of which upstream is silent on.
func ConstIotaBlock() int {
	const (
		unreadFirst = iota + 1 // never read; the two below inherit its expression
		second
		third
	)
	return second + third
}

func ConstSingle() int {
	const unread = 1
	return 2
}

func ConstTyped() string {
	const unread string = "x"
	return "y"
}

func ConstMultiName() int {
	const unread, read = 1, 2
	return read
}

func ConstBesideType() int {
	type local struct{ f int }
	const unread = 3
	return local{f: 1}.f
}

// The `var` side of the same statement kind still builds its cell, so a wasted
// assignment next to a const declaration is still reported.
func VarBesideConst(cond bool) int {
	const tag = 1
	var n = 1
	n = 2
	if cond {
		n = 3
	}
	return n
}

// A variable whose address is taken is heap-allocated by go/ssa, and
// `finishBody` drops heap allocs from `Function.Locals` — the set wastedassign
// walks. So none of the shapes below is a finding, however dead the store
// looks: the pointer can write the cell from anywhere.
//
// syncthing `cmd/syncthing/perfstats_unix.go` is EscapesStructInLoop:
// `runtime.ReadMemStats(&prevMem)` before the loop, `prevMem = curMem` at the
// tail of it, and `prevMem` never read.

func fillInt(p *int) { *p = 1 }

type memish struct{ n int }

func fillMemish(p *memish) { p.n = 1 }

func EscapesStructInLoop() {
	var cur, prev memish
	fillMemish(&prev)
	for i := 0; i < 3; i++ {
		fillMemish(&cur)
		_ = cur.n
		prev = cur
	}
}

func EscapesIntInLoop() {
	var cur, prev int
	fillInt(&prev)
	for i := 0; i < 3; i++ {
		fillInt(&cur)
		_ = cur
		prev = cur
	}
}

func EscapesOnce() {
	var prev int
	fillInt(&prev)
	prev = 3
}

func EscapesTwice() {
	var prev int
	fillInt(&prev)
	prev = 3
	prev = 4
}

// The mark is not flow-sensitive, in either tool: the address is taken after
// the wasted store and the store is still not reported.
func EscapesAfterTheWaste() {
	x := 1
	x = 2
	fillInt(&x)
}

// Captured by a closure that reads it — also escaping, also silent.
func CapturedAndRead() func() int {
	x := 1
	x = 2
	return func() int { return x }
}

// Captured by a closure that does *not* mention it: nothing escapes, so the
// wasted store is still a finding. This is the control for the four above.
func CapturedByNothing() int {
	x := 1
	x = 2
	f := func() int { return 7 }
	return f() + x
}

// A field and a slice element are addressed through FieldAddr/IndexAddr, not
// through an Alloc in Locals, so neither tool reports these either.
func FieldStore() memish {
	var b memish
	b.n = 1
	b.n = 2
	return b
}

func ElemStore() []int {
	s := make([]int, 2)
	s[0] = 1
	s[0] = 2
	return s
}

// The `if`-init store whose value the condition reads is excused because
// NaiveForm does not Load it. Keyed on the *object* rather than the store,
// that excuse also covered the declaration above the loop, which has nothing
// to do with the `if` (beats `auditbeat/.../filetree.go:117`).
func IfInitCondRead(tree map[string]map[string]int, components []string) int {
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

// The same shape without the loop, so the two stores are adjacent.
func IfInitTwoStores(m map[string]int) bool {
	v, ok := 0, false
	if v, ok = m["x"]; !ok {
		return false
	}
	return v > 0
}

func trim(s string) string { return s }

// An assignment whose right-hand side mentions the variable: the operands are
// evaluated *before* the store, so that mention is not a later read. Counting
// it by position silenced every self-referencing assignment, a parameter
// normalised on entry among them (beats
// `libbeat/kibana/index_pattern_generator.go:39`).
func SelfReferencingParam(name string) string {
	name = trim(name)
	return "x"
}

func SelfReferencingLocal() string {
	s := "q"
	s = trim(s)
	return "x"
}

func SelfReferencingThenReassigned() string {
	s := "q"
	s = trim(s)
	s = "z"
	return s
}

// The control: a *later* assignment's right-hand side really does read the
// value, so the first store is live.
func SelfReferenceIsARealRead() string {
	s := "q"
	s = trim(s)
	return s
}

// Upstream's `srcFuncs` is every named function in the package's AST, methods
// included. A members-only list leaves out every method in the package —
// beats' `(FileTree).getByComponents` is one, which is why the `if`-init store
// above it went unreported even after the excuse was narrowed.
type Tree map[string]Tree

func (t Tree) MethodWastedStore() int {
	x := 1
	x = 2
	return x
}

func (t *Tree) PointerMethodWastedStore() int {
	x := 1
	x = 2
	return x
}

// The beats shape itself: a method, an `if`-init whose condition reads the
// variable, and a declaration above the loop that has nothing to do with it.
func (t Tree) GetByComponents(components []string) (Tree, error) {
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
type Elem struct{ N int }

func mkElem() Elem          { return Elem{} }
func mkSlice() []int        { return nil }
func mkMap() map[string]int { return nil }
func mkArray() [3]int       { return [3]int{} }

// Silent: a struct literal.
func StructLiteralInit(c bool) Elem {
	e := Elem{}
	if c {
		e = mkElem()
	} else {
		e = mkElem()
	}
	return e
}

// Silent: an array literal, elements and all.
func ArrayLiteralInit(c bool) [3]int {
	a := [3]int{1, 2, 3}
	if c {
		a = mkArray()
	} else {
		a = mkArray()
	}
	return a
}

// Reported, at the `{`.
func SliceLiteralInit(c bool) []int {
	s := []int{1}
	if c {
		s = mkSlice()
	} else {
		s = mkSlice()
	}
	return s
}

func MapLiteralInit(c bool) map[string]int {
	m := map[string]int{"a": 1}
	if c {
		m = mkMap()
	} else {
		m = mkMap()
	}
	return m
}

// `&Elem{}` is a UnaryExpr around the literal, not a literal: an ordinary
// store at the `=`.
func PointerToLiteralInit(c bool) *Elem {
	p := &Elem{}
	if c {
		p = nil
	} else {
		p = nil
	}
	return p
}

// Inside a func literal. `srcFuncs` adds every `AnonFuncs` entry, so upstream
// looks in here — but guff's "captured by a func literal" guard collected every
// `uses` entry in a literal's body, which includes the literal's *own* locals:
// a wasted store always mentions its variable again, so every local of every
// literal was suppressed and nothing in a closure was ever reported.
func Run(f func()) { f() }

func Two() (int, error) { return 0, nil }

func ChanInitInLit(await chan *int) {
	Run(func() {
		c := <-await
		c = nil
		c = <-await
		fmt.Print(c)
	})
}

func ErrChainInLit() {
	Run(func() {
		p, err := Two()
		fmt.Print(p)
		p, err = Two()
		fmt.Print(p, err)
	})
}

// A *parameter* of the literal, compound-assigned and then thrown away — beats'
// `libbeat/reader/debug` makeNullCheck.
func LitParamCompoundAssign() func(int64, []byte) bool {
	return func(offset int64, buf []byte) bool {
		if len(buf) == 0 {
			offset += int64(len(buf))
			return false
		}
		return true
	}
}

// A local of the *outer* literal, wasted there.
func NestedLitLocal(n int) func() int {
	return func() int {
		mid := 0
		mid = n
		mid = n * 2
		return mid
	}
}

// Controls: a local that is *free* in the literal. The store looks dead in the
// enclosing function's NaiveForm SSA because only the closure reads it — the
// shape the capture guard exists for (traefik's `bodySize`).
func OkFreeInLit(n int) func() int {
	size := 0
	size = n
	return func() int { return size }
}

func OkFreeInGo(n int) {
	size := 0
	size = n
	go func() { _ = size }()
}

func OkFreeInNestedLit(n int) func() func() int {
	return func() func() int {
		mid := 0
		mid = n
		return func() int { return mid }
	}
}

// A read the store cannot reach. The AST fallback is positional, and a position
// says nothing about reachability: beats' `libbeat/reader/debug` makeNullCheck
// puts the store in a block that ends in `return`, and the `offset` on the line
// after that block made the fallback call the store live.
func NullCheckInLit(pattern []byte) func(int64, []byte) bool {
	return func(offset int64, buf []byte) bool {
		if len(buf) < len(pattern) {
			offset += int64(len(buf))
			return false
		}
		fmt.Print(offset + 1)
		return true
	}
}

func NullCheckParam(pattern []byte, offset int64, buf []byte) bool {
	if len(buf) < len(pattern) {
		offset += int64(len(buf))
		return false
	}
	fmt.Print(offset + 1)
	return true
}

// The `return` one block out still cuts the function off.
func ReturnInOuterBlock(xs []int, c bool) int {
	offset := 0
	if c {
		if len(xs) > 0 {
			offset += 1
		}
		return 0
	}
	fmt.Print(offset)
	return offset
}

// Controls: `break` and `continue` leave a loop but stay in the function, so
// the read after the loop is reachable — only `return` cuts it off.
func OkBreakThenRead(xs []int) int {
	offset := 0
	for _, x := range xs {
		if x == 0 {
			offset += 1
			break
		}
	}
	return offset
}

func OkReadInsideBeforeReturn(buf []byte) int64 {
	var offset int64
	if len(buf) == 0 {
		offset += int64(len(buf))
		return offset
	}
	return 0
}
