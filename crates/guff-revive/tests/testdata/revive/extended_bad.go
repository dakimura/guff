// Package util exercises revive extended rules.
package util

import (
	"errors"
	"fmt"
	"runtime"
	"sort"
	"sync"
	"sync/atomic"
	"time"
)

import "os"
import osdup "os"
import dot "example.com/revive/dot"
import BadAlias "example.com/revive/badalias"

var counter int64

func badAtomic() {
	counter = atomic.AddInt64(&counter, 1)
}

func badBareReturn() (n int) {
	return
}

func badBoolLiteral() bool {
	return true || false
}

func badGc() {
	runtime.GC()
}

func badComplexity(x int) int {
	if x > 0 {
		if x > 1 {
			if x > 2 {
				if x > 3 {
					if x > 4 {
						if x > 5 {
							if x > 6 {
								if x > 7 {
									if x > 8 {
										if x > 9 {
											return x
										}
									}
								}
							}
						}
					}
				}
			}
		}
	}
	return 0
}

func badUseErrorsNew() error {
	return fmt.Errorf("plain")
}

func badWaitGroup(wg sync.WaitGroup) {}

func badStringOfInt() string {
	return string(42)
}

func badTimeEqual(a, b time.Time) bool {
	return a == b
}

func badTypeAssert(v any) int {
	return v.(int)
}

func badTypeAssertIgnored(v any) int {
	x, _ := v.(int)
	return x
}

func badRecursion() {
	badRecursion()
}

func badIfReturn() error {
	if err := errors.New("x"); err != nil {
		return err
	}
	return nil
}

func badUnnecessaryFormat() {
	fmt.Printf("hello")
}

func badImportShadow() {
	fmt := 1
	_ = fmt
}

func badConstLogical(a int) bool {
	return a == a
}

func badTimeDate() time.Time {
	return time.Date(2023, 0, 15, 25, 70, 61, 1e9, nil)
}

func badUnhandledError() {
	errors.New("x")
}

// localError is called for its error and ignored below. The callee has to live
// in this package: revive type-checks with an importer that resolves nothing,
// so it cannot see that errors.New above returns an error at all, and reports
// only calls it can type (see crates/guff-revive/src/rules/unhandled_error.rs).
func localError() error { return nil }

func badUnhandledLocalError() {
	localError()
}

type badStructTag struct {
	name string `json:"name,unknownopt"`
	private string `json:"private"`
}

func badUnnecessaryStmt() {
	return
}

func badArgLimit(a, b, c, d, e, f, g, h, i int) {}

func badAddConstant() {
	_ = 42
	_ = "repeat"
	_ = "repeat"
	_ = "repeat"
}

func badEarlyReturn(x int) {
	if x > 0 {
		fmt.Print("big")
	} else {
		return
	}
}

func badDeepExit() {
	os.Exit(1)
}

func GetValue() {}

func badUnnecessaryIf(flag bool) bool {
	if flag {
		return true
	} else {
		return false
	}
}

func badDefer() {
	for i := 0; i < 10; i++ {
		defer fmt.Print(i)
	}
	recover()
	_ = dot.X
}

func badFlagParameter(enabled bool) {
	if enabled {
		fmt.Print("on")
	}
}

func badFunctionResults() (int, int, int, int) {
	return 0, 0, 0, 0
}

func badUseAny(v interface{}) {}

func badUseFmtPrint() {
	print("hello")
}

type badUnusedReceiver struct{}

func (r badUnusedReceiver) noop() {
	fmt.Print("x")
}

func badModifiesParameter(x int) {
	x = 1
}

func badIdenticalBranches(x int) {
	if x > 0 {
		fmt.Print("same")
	} else {
		fmt.Print("same")
	}
}

func badIdenticalIfElseIf(x int) {
	if x == 1 {
		fmt.Print("same")
	} else if x == 2 {
		fmt.Print("same")
	}
}

func badIdenticalIfElseIfCond(x int) {
	if x > 0 {
		fmt.Print("a")
	} else if x > 0 {
		fmt.Print("b")
	}
}

func badIdenticalSwitch(x int) {
	switch x {
	case 1:
		fmt.Print("same")
	case 2:
		fmt.Print("same")
	}
}

func badIdenticalSwitchCond() {
	switch {
	case true:
		fmt.Print("a")
	case true:
		fmt.Print("b")
	}
}

var badLongLine = "012345678901234567890123456789012345678901234567890123456789012345678901234567890"

func badMaxControlNesting() {
	if true {
		if true {
			if true {
				if true {
					if true {
						if true {
							fmt.Print("deep")
						}
					}
				}
			}
		}
	}
}

type badNestedStruct struct {
	inner struct{ x int }
}

func badUnexportedNaming() {
	ExportedLocal := 1
	_ = ExportedLocal
}

func badEmptyLines() {

	fmt.Print("x")

}

func badOptimizeOperands(flag bool) bool {
	return expensive() && flag
}

func expensive() bool {
	return true
}

func badRangeValInClosure() {
	for i := range []int{1} {
		go func() { fmt.Print(i) }()
	}
}

func badConfusingResults() (int, int) {
	return 0, 0
}

func badFunctionLength() int {
	var a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t int
	_ = a
	_ = b
	_ = c
	_ = d
	_ = e
	_ = f
	_ = g
	_ = h
	_ = i
	_ = j
	_ = k
	_ = l
	_ = m
	_ = n
	_ = o
	_ = p
	_ = q
	_ = r
	_ = s
	_ = t
	_ = a
	_ = b
	_ = c
	_ = d
	_ = e
	_ = f
	_ = g
	_ = h
	_ = i
	_ = j
	_ = k
	_ = l
	_ = m
	_ = n
	_ = o
	_ = p
	_ = q
	_ = r
	_ = s
	_ = t
	_ = a
	_ = b
	_ = c
	_ = d
	_ = e
	_ = f
	_ = g
	_ = h
	_ = i
	_ = j
	_ = k
	_ = l
	_ = m
	_ = n
	_ = o
	_ = p
	_ = q
	_ = r
	_ = s
	return t
}

//no space comment
var badURL = "http://example.com/path"

type confusingNaming struct{}

func (c confusingNaming) Foo() {}
func (c confusingNaming) foo() {}

type confusingFields struct {
	Foo int
	foo int
}

type valueRecv struct{ n int }

func (v valueRecv) badModify() { v.n = 1 }

func badUselessBreak(x int) {
	switch x {
	case 1:
		fmt.Print("a")
		break
	}
}

// A single case listing several expressions is not an if-then, so upstream
// skips it (`if len(cc.List) > 1 { return }`) and unnecessary-stmt is silent.
// coredns writes four of these.
func okSwitchOneCaseManyExprs(x int) {
	switch x {
	case 1, 2, 3:
		fmt.Print("c")
	}
}

func badUselessFallthrough(x int) {
	switch x {
	case 1:
		fallthrough
	case 2:
		fmt.Print("b")
	}
}

func badMapLookup(m map[string]int) {
	for k := range m {
		if k == "target" {
			return
		}
	}
}

func badRangeAddr() {
	xs := []int{1, 2}
	for _, v := range xs {
		_ = &v
	}
}

func badMultilineIfInit() {
	if x := struct {
		a int
	}{
		a: 1,
	}; x.a > 0 {
		fmt.Print(x.a)
	}
}

func badSlicesSort() {
	nums := []int{1, 2}
	sort.Ints(nums)
}

func badStringFormat() {
	fmt.Println("INVALID")
}

func badEpochNaming() {
	t := time.Now()
	x := t.Unix()
	_ = x
}

var badK = 1

var badEnforceMap = map[string]int{}

var badEnforceSlice = []int{}

func badDataRaceNamed() (result int) {
	go func() {
		result = 1
	}()
	return result
}

func badEnforceRepeatedArg(a int, b int) int {
	return a + b
}

func badForbiddenWgGo() {
	var wg sync.WaitGroup
	wg.Go(func() {
		wg.Done()
	})
}

// The two import lines above exist to make `duplicated-imports` fire: revive
// keys that rule on the import *path* alone, so an alias still counts as a
// duplicate — and unlike a bare second `import "os"`, an aliased one compiles.
// Go rejects an unused import, so both names have to be referenced; keeping
// the references at the end of the file leaves every position above unmoved.
var _ = osdup.Args

var _ = BadAlias.X

// A comment anywhere between the `if` and the `return nil` is revive's signal
// that the construct is deliberate (`containsComments`), so the pair below is
// not a finding even though it is the same shape as `badIfReturn` above.
func okIfReturnWithComment() error {
	if err := errors.New("x"); err != nil {
		return err
	}
	// Return nil when there was nothing to report.
	return nil
}

const nestingZero = 0

// `for … range` is not a nesting level for max-control-nesting: the visitor
// switches on IfStmt / ForStmt / CaseClause / CommClause / FuncLit and a
// `*ast.RangeStmt` falls through to the default, which descends with the
// counter untouched. Six of them plus an `if` is one level, not seven.
// (jaeger internal/storage/v2/memory, internal/storage/metricstore.)
func okRangeIsNotNesting(xs [][][][][][]int) int {
	total := nestingZero
	for _, a := range xs {
		for _, b := range a {
			for _, c := range b {
				for _, d := range c {
					for _, e := range d {
						for _, f := range e {
							if f > nestingZero {
								total += f
							}
						}
					}
				}
			}
		}
	}
	return total
}

// A three-clause `for` does count, so this one is over the limit even though
// the `range` loop around it is not.
func badMixedNesting(xs [][]int, n int) int {
	total := nestingZero
	for _, a := range xs {
		for i := nestingZero; i < n; i++ {
			for j := nestingZero; j < n; j++ {
				for k := nestingZero; k < n; k++ {
					for l := nestingZero; l < n; l++ {
						for m := nestingZero; m < n; m++ {
							if m > nestingZero {
								total += a[i] + a[j] + a[k] + a[l] + m
							}
						}
					}
				}
			}
		}
	}
	return total
}

// unnecessary-if compares the two assignment targets **as text** — upstream
// renders both with `astutils.GoFmt` and bails when they differ. A renderer
// that answers a placeholder for the shapes it does not know makes two
// different targets compare equal: fiber's
// `if colors { cfg.ForceColors = true } else { cfg.DisableColors = true }`
// became a finding that way, and `if n > 1` reported `b = n > <expr>`.

type unnecessaryIfCfg struct {
	Force   bool
	Disable bool
}

// Reported.
func unnecessaryIfSameField(c bool, s *unnecessaryIfCfg) {
	if c {
		s.Force = true
	} else {
		s.Force = false
	}
}

func unnecessaryIfRelational(n int) bool {
	var b bool
	if n > 1 {
		b = true
	} else {
		b = false
	}

	return b
}

func unnecessaryIfNegatedRelational(n int) bool {
	var b bool
	if n > 1 {
		b = false
	} else {
		b = true
	}

	return b
}

// Silent: the two branches assign to different fields.
func unnecessaryIfDifferentFields(c bool, s *unnecessaryIfCfg) {
	if c {
		s.Force = true
	} else {
		s.Disable = true
	}
}

// Silent: the right-hand sides are not boolean literals.
func unnecessaryIfNonBool(c bool) int {
	var x int
	if c {
		x = 1
	} else {
		x = 0
	}

	return x
}

// datarace keys on the *identity* of the declared name, not on the name.
// Upstream compares `*ast.Object`s, so any inner declaration that reuses an
// outer name is a different object and never matches. Comparing the text made
// every shadowing declaration a capture — fiber's
// `func(addr net.Addr) { addrChan <- addr.String() }`, two closures deep inside
// a function whose named result is also `addr`, drew two findings.

func dataraceSink(int)            {}
func dataraceSinkS(string)        {}
func dataraceTake(f func(string)) {}

// Reported: the named result itself is captured.
func dataraceNamedResult() (x int) {
	go func() { dataraceSink(x) }()

	return
}

// Reported: captured from inside a nested closure that does not shadow it.
func dataraceNestedNoShadow() (addr string) {
	go func() { dataraceTake(func(s string) { dataraceSinkS(addr + s) }) }()

	return
}

// Reported: one of two named results.
func dataraceSecondResult() (a, b int) {
	go func() { dataraceSink(b) }()

	return
}

// Silent: the goroutine's own parameter shadows the result.
func dataraceParamShadows() (x int) {
	go func(x int) { dataraceSink(x) }(1)

	return
}

// Silent: a local declared inside the goroutine shadows it.
func dataraceLocalShadows() (x int) {
	go func() {
		x := 1
		dataraceSink(x)
	}()

	return
}

// Silent: a nested closure's parameter shadows it — fiber's shape.
func dataraceNestedShadows() (addr string) {
	go func() { dataraceTake(func(addr string) { dataraceSinkS(addr) }) }()

	return
}

// Silent: the result is not named.
func dataraceUnnamedResult() int {
	x := 0
	go func() { dataraceSink(x) }()

	return x
}

// Silent: the goroutine is not a function literal.
func dataraceNotAFuncLit() (x int) {
	go dataraceSink(x)

	return
}
