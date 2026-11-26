package errorlint_errorf

// The `fmt.Errorf` half of errorlint, plus the two report positions and the
// suppression guard the same grid turned up in the type-assertion half.
//
// **golangci-lint pins this check on.** The analyzer ships `errorf` *off*
// (`a.Flags.BoolVar(&checkErrorf, "errorf", false, …)`), and golangci-lint
// seeds `ErrorLint{Errorf: true, ErrorfMulti: true, Asserts: true,
// Comparison: true}` and forwards all four every run. guff read the analyzer's
// default, so the half never ran.
//
// Every shape below was measured against golangci-lint 2.12.2 (go-errorlint
// v1.9.0). `// FINDING` marks a reported line; the position is the *argument*,
// not the call. Note that the default `issues.uniq-by-line` is true and would
// hide the second finding on the nested-call line — the golden case sets it
// false.

import (
	"errors"
	"fmt"
)

var ErrSentinel = errors.New("sentinel")

type myErr struct{}

func (myErr) Error() string { return "my" }

func pipelineShape(err error) error { return fmt.Errorf("%w: %v", ErrSentinel, err) } // FINDING

func plainV(err error) error { return fmt.Errorf("%v", err) } // FINDING

// `%w` is the point of the check: silent.
func plainW(err error) error { return fmt.Errorf("%w", err) }

func plainS(err error) error { return fmt.Errorf("%s", err) } // FINDING

// Two wraps are legal since Go 1.20, and `errorf-multi` is on by default.
func twoWraps(a, b error) error { return fmt.Errorf("%w: %w", a, b) }

func intThenErr(err error) error { return fmt.Errorf("%d %v", 1, err) } // FINDING

// The argument is not an error.
func notAnError(s string) error { return fmt.Errorf("%v", s) }

// A format string that is not a literal is skipped: `printfFormatStringVerbs`
// wants an `*ast.BasicLit`.
func nonLiteralFormat(f string, err error) error { return fmt.Errorf(f, err) }

// `len(call.Args) <= 1`.
func noArgs() error { return fmt.Errorf("no verbs") }

// `%T` is skipped alongside `%w`.
func typeVerb(err error) error { return fmt.Errorf("%T", err) }

// An explicit `[n]` sets the argument index, so `%[2]v` names the error.
func indexedVerbs(err error) error { return fmt.Errorf("%[2]v %[1]d", 1, err) } // FINDING

// `%%` restarts the parse and consumes no argument.
func escapedPercent(err error) error { return fmt.Errorf("100%% %v", err) } // FINDING

func plusV(err error) error { return fmt.Errorf("%+v", err) } // FINDING

// The parser does not know `-`, so it ends the verb: this parses as the verb
// `-`, which is neither `w` nor `T`, and the call is still reported.
func dashWidth(err error) error { return fmt.Errorf("%-10v", err) } // FINDING

// Two offending verbs, one diagnostic — at the first argument, carrying two
// suggested fixes.
func twoPlainErrs(a, b error) error { return fmt.Errorf("%v: %v", a, b) } // FINDING

func customType(e myErr) error { return fmt.Errorf("%v", e) } // FINDING

func wrapThenPlain(a, b error) error { return fmt.Errorf("%w then %v", a, b) } // FINDING

func plainThenWrap(a, b error) error { return fmt.Errorf("%v then %w", a, b) } // FINDING

// A verb past the end of the argument list is skipped, not counted.
func moreVerbsThanArgs(err error) error { return fmt.Errorf("%v %v", err) } // FINDING

func moreArgsThanVerbs(a, b error) error { return fmt.Errorf("%v", a, b) } // FINDING

// Two calls on one line, each with its own error argument: two findings. The
// default `uniq-by-line: true` would show only one.
func nestedCall(err error) error { return fmt.Errorf("%v", fmt.Errorf("%v", err)) } // FINDING x2

func precision(err error) error { return fmt.Errorf("%.3v", err) } // FINDING

func sharpVerb(err error) error { return fmt.Errorf("%#v", err) } // FINDING

type ptrErr struct{}

func (*ptrErr) Error() string { return "p" }

// --- the type-assertion half, whose positions the same grid corrected ---

// Reported at the comparison.
func plainComparison(err error) bool { return err == ErrSentinel } // FINDING

// `typeAssert.Pos()` is the `X`, so the column is `err` — guff pointed at the
// `(` four columns over.
func plainAssert(err error) bool { _, ok := err.(*ptrErr); return ok } // FINDING

// Likewise for a type switch: the position is the switched expression, not
// the `switch` keyword.
func plainTypeSwitch(err error) int {
	switch err.(type) { // FINDING
	case *ptrErr:
		return 1
	}
	return 0
}

type stringerIface interface{ String() string }

// guff had an "and some case must implement error" guard that upstream does
// not have. It silenced this shape and the `case nil:` one below.
func switchNonErrorCase(err error) int {
	switch err.(type) { // FINDING
	case stringerIface:
		return 1
	}
	return 0
}

// With a binding, the position is still the switched expression.
func switchWithBinding(err error) int {
	switch e := err.(type) { // FINDING
	case *ptrErr:
		_ = e
		return 1
	}
	return 0
}

// A `case nil:` clause is no exemption either.
func switchNilCase(err error) int {
	switch err.(type) { // FINDING
	case nil:
		return 1
	}
	return 0
}
