// Package extendedtest is a clean fixture for revive extended rules.
package extendedtest

import (
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"time"
)

const (
	zero       = 0
	one        = 1
	two        = 2
	year2023   = 2023
	monthJan   = 1
	day15      = 15
	hour12     = 12
	minute30   = 30
	second45   = 45
	nanosecond = 0
	labelA     = "a"
	labelB     = "b"
	labelOne   = "one"
	labelTwo   = "two"
	labelOk    = "ok"
	labelX     = "x"
	labelBig   = "big"
)

var counter int64

func goodAtomic() {
	atomic.AddInt64(&counter, one)
}

func goodReturn() int {
	return one
}

func goodBoolLiteral(x bool) bool {
	return x
}

func goodWaitGroup(wg *sync.WaitGroup) {
	wg.Add(one)
}

func goodStringOfInt(n int) string {
	_ = n
	return "42"
}

func goodTimeEqual(a, b time.Time) bool {
	return a.Equal(b)
}

func goodTypeAssert(v any) (int, bool) {
	x, ok := v.(int)
	return x, ok
}

func goodRecursion(n int) int {
	if n <= zero {
		return zero
	}
	return goodRecursion(n - one)
}

func goodIfReturn() error {
	return errors.New("x")
}

func goodFormat() {
	_, _ = fmt.Print("hello")
}

func goodImportShadow() string {
	return fmt.Sprintf("%s", "ok")
}

func goodConstLogical(a int) bool {
	return a > zero
}

func goodTimeDate() time.Time {
	return time.Date(
		year2023,
		monthJan,
		day15,
		hour12,
		minute30,
		second45,
		nanosecond,
		time.UTC,
	)
}

func goodUnhandledError() error {
	return errors.New("x")
}

type goodStructTag struct {
	Name string `json:"name,omitempty"`
}

func goodUnnecessaryStmt() int {
	return one
}

func goodArgLimit(a, b, c int) int {
	return a + b + c
}

func goodAddConstant() string {
	return "once"
}

func goodEarlyReturn(x int) {
	if x <= zero {
		return
	}
	_, _ = fmt.Print(labelBig)
}

func goodDeepExit() {
	_, _ = fmt.Print(labelOk)
}

func getValue() int {
	return one
}

func goodUnnecessaryIf(flag bool) bool {
	return flag
}

func goodDefer() {
	defer func() {
		_ = recover()
	}()
}

func goodFlagParameter(enabled bool) {
	_ = enabled
}

func goodFunctionResults() (int, error) {
	return zero, nil
}

func goodUseAny(v any) {
	_ = v
}

func goodUseFmtPrint() {
	_, _ = fmt.Print("hello")
}

type goodUnusedReceiver struct{}

func (r *goodUnusedReceiver) used() {
	_ = r
	_, _ = fmt.Print(labelOk)
}

func goodModifiesParameter(x int) int {
	return x + one
}

func goodIdenticalBranches(x int) {
	if x > zero {
		_, _ = fmt.Print(labelA)
	} else {
		_, _ = fmt.Print(labelB)
	}
}

func goodIdenticalIfElseIf(x int) {
	if x == one {
		_, _ = fmt.Print(labelOne)
	} else if x == two {
		_, _ = fmt.Print(labelTwo)
	}
}

func goodIdenticalIfElseIfCond(x int) {
	if x > zero {
		_, _ = fmt.Print(labelA)
	} else if x < zero {
		_, _ = fmt.Print(labelB)
	}
}

func goodIdenticalSwitch(x int) {
	switch x {
	case one:
		_, _ = fmt.Print(labelOne)
	case two:
		_, _ = fmt.Print(labelTwo)
	}
}

func goodIdenticalSwitchCond(x int) {
	switch {
	case x > zero:
		_, _ = fmt.Print(labelA)
	case x < zero:
		_, _ = fmt.Print(labelB)
	}
}

func goodMaxControlNesting() {
	if true {
		_, _ = fmt.Print(labelOk)
	}
}

type goodNestedStruct struct {
	x int
}

func goodUnexportedNaming() {
	local := one
	_ = local
}

func goodEmptyLines() {
	_, _ = fmt.Print(labelX)
}

func goodOptimizeOperands(flag bool) bool {
	return flag && expensiveOk()
}

func expensiveOk() bool {
	return true
}

func goodRangeValInClosure() {
	items := []int{one}
	for _, item := range items {
		captured := item
		go func() { _, _ = fmt.Print(captured) }()
	}
}

func goodConfusingResults() (count int, err error) {
	return zero, nil
}

func goodFunctionLength() int {
	return one
}
