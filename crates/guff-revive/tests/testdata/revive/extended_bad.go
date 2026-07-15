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
import "os"
import dot "dot"
import BadAlias "example.com/badalias"

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
