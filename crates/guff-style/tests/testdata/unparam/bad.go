package unparam

import (
	"log"
	"os"
	"runtime"
	"syscall"
	"testing"
)

type secKind int

const statusOK = 200

var errNotFound = newErr()

func newErr() error { return nil }

func example(used int, unused string) int {
	return used + 1
}

func withBlank(_ int, y int) int {
	return y
}

func stub(unused int) {
	panic("not implemented")
}

func discardOnly(unused int) {
	_ = unused
}

func ExportedUnused(x int) {}

// The three families the SSA half of the check reports.

// `result N is always X`: every return gives the second result the same
// constant. gitea's `getStorageSectionByType` is this shape.
func sectionByType(name string) (string, secKind, error) {
	if name == "" {
		return "", 0, errNotFound
	}
	if name == "x" {
		return name, 0, nil
	}
	return "", 0, errNotFound
}

// `result N is never used`: no call site reads the first result, and at least
// two ignore it.
func saveBlob(data string) (string, error) {
	if data == "" {
		return "", errNotFound
	}
	return data + "!", nil
}

func useSaveBlob() error {
	_, err := saveBlob("a")
	if err != nil {
		return err
	}
	_, err = saveBlob("b")
	return err
}

// `param always receives X`: four call sites, all passing the same constant.
// Reported even though the body uses it.
func xmlResponse(status int, obj string) string {
	if status > 0 {
		return obj
	}
	return ""
}

func useXML() string {
	return xmlResponse(statusOK, "a") + xmlResponse(statusOK, "b") +
		xmlResponse(statusOK, "c") + xmlResponse(statusOK, "d")
}

func useSectionByType(typ string) (string, secKind, error) {
	sec, kind, err := sectionByType(typ)
	if sec != "" || err != nil {
		return sec, kind, err
	}
	return "", 0, nil
}

func runFn(f func()) {}

// Upstream names a function literal after its enclosing function, the way
// go/ssa does — `litIIFE$1`, and `$1$1` for one nested inside another. guff
// printed the placeholder "<func literal>", which golangci-lint can never
// emit, so every such finding was a guaranteed mismatch; nothing caught it
// because this fixture had no literal in it at all.
func litIIFE() {
	_ = func(used int, unused string) int { return used + 1 }(1, "x")
}

// Silent in both: a literal passed as an argument has its signature fixed by
// the callee.
func litAsArg() {
	runFn(func() {})
}

// Silent in both: `go` and `defer` fix the signature the same way.
func litGoDefer() {
	go func(unusedGo int) {}(1)
	defer func(unusedDefer int) {}(2)
}

// --- Statements upstream's IR never reaches -------------------------------
//
// `buildssa` gives go/ssa the `ctrlflow` no-return predicate, so a static call
// to a function that cannot return is followed by a `Panic` and the rest of the
// block is dropped. A parameter used only down there is unused. Every shape
// below was measured against golangci-lint 2.12.2, one terminator at a time;
// `noop()` comes first so `dummyImpl` cannot call the body a stub.

func noop() {}

func afterOsExit(unused bool) {
	noop()
	os.Exit(1)
	if unused {
		println(1)
	}
}

func afterSyscallExit(unused bool) {
	noop()
	syscall.Exit(1)
	if unused {
		println(1)
	}
}

func afterGoexit(unused bool) {
	noop()
	runtime.Goexit()
	if unused {
		println(1)
	}
}

func afterLogFatalf(unused bool) {
	noop()
	log.Fatalf("x")
	if unused {
		println(1)
	}
}

func afterLogPanicln(unused bool) {
	noop()
	log.Panicln("x")
	if unused {
		println(1)
	}
}

var logger = log.New(os.Stderr, "", 0)

func afterLoggerFatal(unused bool) {
	noop()
	logger.Fatal("x")
	if unused {
		println(1)
	}
}

func afterTestingFatal(t *testing.T, unused bool) {
	noop()
	t.Fatal("x")
	if unused {
		println(1)
	}
}

func afterTestingSkip(t *testing.T, unused bool) {
	noop()
	t.Skip("x")
	if unused {
		println(1)
	}
}

func afterTestingSkipNow(t *testing.T, unused bool) {
	noop()
	t.SkipNow()
	if unused {
		println(1)
	}
}

// The terminator does not have to be the first statement.
func afterNestedSkip(t *testing.T, unused bool) {
	if t == nil {
		println(0)
	}
	t.Skip("x")
	if unused {
		println(1)
	}
}

// One hop of in-package induction: `dies` is no-return, so `afterDies` is too.
func dies() {
	noop()
	os.Exit(1)
}

func afterDies(unused bool) {
	dies()
	if unused {
		println(1)
	}
}

// Two hops.
func diesTwice() {
	dies()
}

func afterDiesTwice(unused bool) {
	diesTwice()
	if unused {
		println(1)
	}
}

// Call sites in dead code are not call sites: `ssautil.AllFunctions` cannot
// reach a block `deleteUnreachableBlocks` removed. Only the four `"sh"` calls
// below survive, which is exactly upstream's threshold — thanos'
// `examples/interactive` is this shape.
func execCmd(cmd string, args ...string) {
	println(cmd, len(args))
}

func execCmdSites() {
	execCmd("sh", "-c", "a")
	execCmd("sh", "-c", "b")
	execCmd("sh", "-c", "c")
	execCmd("sh", "-c", "d")
}

func execCmdSkipped(t *testing.T) {
	t.Skip("interactive")
	execCmd("cp", "-r", "x")
	execCmd("cp", "-r", "y")
}

// Control flow that does end: an endless `for`, a `select` with no `default`,
// and a `switch` whose every clause ends — each makes its caller's parameter
// unreachable.
func loopsForever() {
	for {
		println(1)
	}
}

func afterLoopsForever(unused bool) {
	loopsForever()
	if unused {
		println(1)
	}
}

var ch chan int

func selectDies() {
	select {
	case <-ch:
		os.Exit(1)
	}
}

func afterSelectDies(unused bool) {
	selectDies()
	if unused {
		println(1)
	}
}

func switchAllDie(n int) {
	switch n {
	case 1:
		os.Exit(1)
	default:
		panic("x")
	}
}

func afterSwitchAllDie(unused bool) {
	switchAllDie(1)
	if unused {
		println(1)
	}
}
