package main

import (
	"fmt"
	"log"
	"os"
	"testing"
)

func fn(s string) {
	fmt.Printf(s)
}

func main() { fn("x") }

// Eight of the fifteen rows in upstream's list are *methods*, and their
// `(Symbol "…")` is spelled the way `typeutil.FuncName` spells one. Package
// path plus object name — what `is_call_to` answers — turns `t.Errorf` into
// `testing.Errorf`, which is in no list anywhere, so this half of the check
// was unreachable while `fmt.Printf` above kept the golden green.

type svc struct{ logger *log.Logger }

func pkgFuncs(s string, w *os.File) {
	fmt.Printf(s)
	_ = fmt.Sprintf(s)
	_ = fmt.Errorf(s)
	log.Printf(s)
	log.Fatalf(s)
	log.Panicf(s)
	fmt.Fprintf(w, s)
}

func methods(l *log.Logger, t *testing.T, tb testing.TB, sv svc, s string) {
	l.Printf(s)
	t.Errorf(s)
	t.Fatalf(s)
	t.Logf(s)
	t.Skipf(s)
	tb.Errorf(s)
	tb.Logf(s)
	sv.logger.Printf(s)
}

func silent(s string, n int, w *os.File) {
	// Further arguments: `format:[]` is an exact arity.
	fmt.Printf(s, n)
	// A constant format string is not dynamic.
	fmt.Printf("hello")
	// `fmt.Sprintln` is not in the list.
	_ = fmt.Sprintln(s)
	fmt.Fprintf(w, s, n)
}
