// Package extendedtest exercises revive extended rules.
package extendedtest

import (
	"errors"
	"fmt"
	"runtime"
	"sync"
	"sync/atomic"
	"time"
)

import "os"
import "os"
import dot "dot"

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
