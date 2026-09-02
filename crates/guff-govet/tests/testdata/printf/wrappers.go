package wrappers

// Which functions are printf-like. Every shape here was run through
// golangci-lint 2.12.2 and guff side by side before it was written down; the
// comment on each says which of the two used to be wrong.

import (
	"fmt"
	"testing"
)

var err error

// A method whose name ends in `f` but whose body forwards `args` as a slice,
// not as `args...` — the shape of `(*zap.SugaredLogger).Panicf`, which stood
// behind fifteen guff-only findings on Tekton pipeline. Not a wrapper: silent.
type sugar struct{}

func (s *sugar) log(lvl int, template string, args []any) {
	fmt.Printf("%d %s %v", lvl, template, args)
}

func (s *sugar) Panicf(template string, args ...any) {
	s.log(0, template, args)
}

func notAWrapperMethod(s *sugar) { s.Panicf("a %w", err) }

// A real wrapper: its kind is deduced from the body, and `%w` is a diagnostic
// because `fmt.Printf` is KindPrintf, not KindErrorf.
func wrapf(format string, args ...any) {
	fmt.Printf(format, args...)
}

func wrapperTakesW() { wrapf("b %w", err) }

// A wrapper around `fmt.Errorf` is KindErrorf, so `%w` is legal here.
func wrapErrorf(format string, args ...any) error {
	return fmt.Errorf(format, args...)
}

func errorfWrapperAllowsW() { _ = wrapErrorf("c %w", err) }

// Two methods with printf-ish names and bodies that forward nothing. guff's
// old name heuristic reported both; upstream never looked at them.
type quiet struct{}

func (q quiet) Errorf(format string, args ...any) {
	_ = format
	_ = args
}

func (q quiet) Printf(format string, args ...any) {
	_ = format
	_ = args
}

func quietMethods(q quiet) {
	q.Errorf("d %z", 1)
	q.Printf("i %z", 1)
}

// Forwarding without `...` is the mistake the user probably made, so it is
// reported — and the candidate does *not* become a wrapper.
func badForward(format string, args ...any) {
	fmt.Printf(format, args)
}

func callsBadForward() { badForward("e %d", 1) }

// An unknown verb reached through a wrapper.
func unknownVerbThroughWrapper() { wrapf("f %z", 1) }

// A function literal in a variable is a candidate too, and it is named by the
// variable — `litf`, not by whatever it forwards to.
var litf = func(format string, args ...any) {
	fmt.Printf(format, args...)
}

func callsLiteral() { litf("j %z", 1) }

// The kind travels back through a chain of wrappers.
func hop1(format string, args ...any) { wrapf(format, args...) }

func callsTwoHop() { hop1("k %z", 1) }

// A candidate that reassigns `args` before forwarding is not a simple wrapper.
func mutatesArgs(format string, args ...any) {
	args = args[1:]
	fmt.Printf(format, args...)
}

func callsMutatesArgs() { mutatesArgs("l %z", 1, 2) }

// Nor is one that takes the address of `args`.
func addrOfArgs(format string, args ...any) {
	_ = &args
	fmt.Printf(format, args...)
}

func callsAddrOfArgs() { addrOfArgs("m %z", 1) }

// `(*testing.common).Errorf` is in the allowlist and ends in `f`, so it is
// KindPrintf — *not* KindErrorf. guff used to answer "errorf" for any name
// ending in `Errorf` and let `%w` through here.
func testingErrorfRejectsW(t *testing.T) { t.Errorf("n %w", err) }

// An unformatted wrapper: the missing `...` is reported as `print-like`.
func printWrapper(args ...any) {
	fmt.Println(args)
}

func callsPrintWrapper() { printWrapper(1) }

// A wrapper that returns a string rather than printing one.
func sprintfWrapper(format string, args ...any) string {
	return fmt.Sprintf(format, args...)
}

func callsSprintfWrapper() { _ = sprintfWrapper("p %z", 1) }

// A function literal assigned to a struct field, named by the field.
type holder struct {
	logf func(format string, args ...any)
}

func fieldLiteral(h *holder) {
	h.logf = func(format string, args ...any) {
		fmt.Printf(format, args...)
	}
	h.logf("q %z", 1)
}

// The format parameter has to be exactly the predeclared `string`; a defined
// type whose underlying type is `string` does not make this a printf wrapper.
type fmtstr string

func namedFormat(format fmtstr, args ...any) {
	fmt.Printf(string(format), args...)
}

func callsNamedFormat() { namedFormat("r %z", 1) }

// A well-formed forward to a print function: nothing to report, because the
// call site of a KindPrint function is not inspected.
func printlnWrapper(args ...any) {
	fmt.Println(args...)
}

func callsPrintlnWrapper() { printlnWrapper(1) }
