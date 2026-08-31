package unparam

import (
	"log"
	"os"
	"testing"
)

type kind2 int

var errNotFound2 = newErr2()

func newErr2() error { return nil }

func allUsed(x int, y string) {
	_ = x
	println(y)
}

func explicitKeep(unused int) {
	_ = unused
}

func emptyBody(unused int) {}

func onlyReturn(unused int) {
	return
}

// Used as a value (callback); signature must stay — unused param is OK.
func asCallback(prefix string) {
	println("ok")
}

var callbacks = []func(string){asCallback}

func takesHandler(h func(x int)) { h(1) }

func callWithLit() {
	takesHandler(func(unused int) {
		println("handler")
	})
}

// `dummyImpl`: a body that only returns constants is skipped entirely, which is
// why this is not "result 1 is always nil".
func dummyReturn() (int, error) {
	return 0, nil
}

// Same, through a call the harmless-call pattern allows (`\berrors\b`).
func dummyErrors() (int, error) {
	return 1, errNotFound2
}

func useDummies() {
	_, _ = dummyReturn()
	_, _ = dummyErrors()
	_, _ = dummyReturn()
	_, _ = dummyErrors()
}

// `return f(...)` fixes f's results: the call is part of the return, so the
// second result cannot be dropped.
func fixedResults(name string) (string, kind2, error) {
	if name == "" {
		return "", 0, errNotFound2
	}
	if name == "x" {
		return name, 0, nil
	}
	return "", 0, errNotFound2
}

func forwards(name string) (string, kind2, error) {
	return fixedResults(name)
}

// Exported: the call sites may be anywhere, so no constant-parameter report.
func Respond(status int, msg string) string {
	if status > 0 {
		return msg
	}
	return ""
}

func useRespond() string {
	return Respond(200, "a") + Respond(200, "b") + Respond(200, "c") + Respond(200, "d")
}

// Only three call sites: below upstream's threshold of four.
func threeSites(status int) int { return status }

func useThreeSites() int { return threeSites(7) + threeSites(7) + threeSites(7) }

// --- Dead code that must stay silent --------------------------------------

func noop2() {}

// `testing.TB` is an interface, so there is no static callee and `go/cfg`'s
// `mayReturn` stays conservative: nothing is cut and `unused` is used.
func tbAbort(t testing.TB, unused bool) {
	noop2()
	t.Skip("x")
	if unused {
		println(1)
	}
}

// `log.Fatal` first, with only harmless statements before it: upstream's
// `dummyImpl` sees the inserted `Panic` and calls the whole body a stub, so no
// parameter of it is ever reported.
func logFatalStub(unused bool) {
	log.Fatal("x")
	if unused {
		println(1)
	}
}

// Same for the `panic` builtin.
func panicStub(unused bool) {
	panic("x")
}

// The only `return` is behind the cut, so upstream has no returns to agree
// over — no `result 0 (error) is always nil`.
func deadReturn(t *testing.T) error {
	noop2()
	t.Skip("x")
	return nil
}

// `anyRealUse` looks for `_ = par` in the whole body, live or not: it is the
// developer saying "keep this parameter", and upstream honours it either way.
func deadIntentionalKeep(t *testing.T, unused int) {
	noop2()
	t.Skip("x")
	_ = unused
}

// A call that does return cuts nothing.
func afterOrdinaryCall(unused bool) {
	noop2()
	println(0)
	if unused {
		println(1)
	}
}

// `go/cfg` gives a `goto` a real edge, so the `return` behind `end:` is live
// and `gotoDies` may return. The structural walk cannot follow the edge, so a
// body with a `goto` in it is not classified at all — silence either way.
func gotoDies() {
	goto end
	os.Exit(1)
end:
	return
}

func afterGotoDies(unused bool) {
	gotoDies()
	if unused {
		println(1)
	}
}

// A `for` whose body can `break`, a `range` that may run zero times, and a
// `switch` with no `default` all reach the statement after them, so none of
// these callees is no-return.
func breaksOut() {
	for {
		break
	}
}

func afterBreaksOut(unused bool) {
	breaksOut()
	if unused {
		println(1)
	}
}

func rangeDies() {
	for range []int{} {
		os.Exit(1)
	}
}

func afterRangeDies(unused bool) {
	rangeDies()
	if unused {
		println(1)
	}
}

func switchWithoutDefault(n int) {
	switch n {
	case 1:
		os.Exit(1)
	}
}

func afterSwitchWithoutDefault(unused bool) {
	switchWithoutDefault(1)
	if unused {
		println(1)
	}
}

// A `defer` makes the block "returns" in go/cfg: a deferred `recover` can turn
// the panic into a return.
func deferThenExit() {
	defer noop2()
	os.Exit(1)
}

func afterDeferThenExit(unused bool) {
	deferThenExit()
	if unused {
		println(1)
	}
}
