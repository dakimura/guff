package gocritic

import (
	"log"
	"os"
)

// `exitAfterDefer` walks the whole function body generically — upstream is
// `astutil.Apply`, not a switch over statement kinds — so every construct that
// can hold a call is covered. guff enumerated the kinds it recursed into and
// silently missed the ones nobody had added: `select`, type switches, labelled
// statements and `go`. celestia-node's shrex peer manager is a `log.Fatal`
// inside a `select` inside a `for`.

// --- reported ---------------------------------------------------------------

func eadInSelect(sub chan int, done chan struct{}) {
	defer close(sub)
	defer log.Println("bye")
	for {
		select {
		case _, ok := <-sub:
			if !ok {
				log.Fatal("closed")
				return
			}
		case <-done:
			return
		}
	}
}

func eadInIf(x int) {
	defer log.Println("bye")
	if x == 0 {
		log.Fatal("zero")
	}
}

func eadInTypeSwitch(v any) {
	defer log.Println("bye")
	switch v.(type) {
	case int:
		log.Fatal("int")
	case string:
		_ = v
	}
}

func eadInLabeled() {
	defer log.Println("bye")
loop:
	for {
		log.Fatal("stop")
		break loop
	}
}

func eadInGo() {
	defer log.Println("bye")
	go log.Fatal("in go")
}

func eadInRange(xs []int) {
	defer log.Println("bye")
	for range xs {
		os.Exit(1)
	}
}

// --- silent -------------------------------------------------------------

// `defer os.Exit(…)` is allowed: whether it clutters anything needs the defer
// stack (go-critic #995).
func eadDeferredExit() {
	defer log.Println("bye")
	defer os.Exit(1)
}

// A function literal is not descended into.
func eadInFuncLit() {
	defer log.Println("bye")
	f := func() { log.Fatal("inner") }
	_ = f
}

// Once a defer is pending, an `else` branch is not descended into: control may
// never reach it.
func eadInElse(x int) {
	defer log.Println("bye")
	if x == 0 {
		_ = x
	} else {
		log.Fatal("else")
	}
}

// The defer comes after the call, so the call is not "after" it.
func eadDeferAfter() {
	log.Fatal("before any defer")
	defer log.Println("bye")
}

// Only the four names count.
func eadOtherExit() {
	defer log.Println("bye")
	log.Panic("not in the set")
}

// No defer at all.
func eadNoDefer() {
	log.Fatal("plain")
}
