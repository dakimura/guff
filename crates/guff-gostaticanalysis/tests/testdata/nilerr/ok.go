package nilerr_ok

func do() error { return nil }

func wrap(err error) error { return err }

func ok() error {
	err := do()
	if err != nil {
		return err
	}
	return nil
}

func okNilBranch() error {
	err := do()
	if err == nil {
		return nil
	}
	return err
}

func okUseErr() error {
	err := do()
	if err != nil {
		return wrap(err)
	}
	return nil
}

func logf(format string, args ...any) {}

// Passing the error to anything variadic boxes it into an `any` first, so the
// use is a `MakeInterface` wrapping it — `isUsedInValue` peels that. Without
// the peel the block looks as though it never mentions the error, which is
// what made dapr's 25 `fmt.Sprintf("…: %v", err)` blocks findings.
func okErrBoxedIntoAny() error {
	err := do()
	if err != nil {
		logf("failed: %v", err)
		return nil
	}
	return nil
}

// A function with **no error result at all**. `return nil, false` returns nil
// for a `*int`, and upstream counts only results that implement `error`
// (`errorReturnValues == 0` → not a finding). guff typed the untyped nil as an
// error and reported six of these in jaeger's
// `internal/storage/elasticsearch/esclient/aggregation.go`.
func okNoErrorResult() (*int, bool) {
	err := do()
	if err != nil {
		return nil, false
	}
	n := 1
	return &n, true
}

type writer struct{}

func (writer) Write(p []byte) (int, error) { return len(p), nil }

// `return w.Write(p)` returns whatever the call returns, error included — the
// error is not swallowed. go/ssa returns the *components* of a multi-valued
// call, so the error-typed result is right there in `Return.Results`; guff
// returned the tuple itself, leaving no error-typed result for `isReturnNil`
// to reject, and traefik's `compression_handler.go:242` became a finding.
func okTailCallReturnsTheError(w writer, p []byte) (int, error) {
	err := do()
	if err != nil {
		return w.Write(p)
	}
	return 0, nil
}

func sink() {}

// A `defer` makes go/ssa give the function a recover block, which forces its
// results to stay addressable: `liftAlloc` refuses to lift a result alloc when
// `fn.Recover != nil`, so `return nil` survives as a *load* rather than an
// `*ssa.Const`. nilerr's `isReturnNil` tests for a const and bails, so neither
// of these is reported — however plainly wrong they read.
//
// go/ssa declares a local for *every* result, named or not, and it does so for
// function literals exactly as for declarations. guff gave a FuncLit result
// locals only when they were *named*, so the literal below kept an ssa.Const
// and was reported where upstream is silent.
func deferredInDecl(e error) error {
	defer sink()
	if e != nil {
		return nil
	}
	return e
}

// The literal has to sit inside a FuncDecl: one in a package-level `var`
// initializer belongs to the synthesized package `init`, which buildssa never
// puts in SrcFuncs — so nilerr would be silent there for an unrelated reason
// and the case would measure nothing.
func deferredInLit() func(error) error {
	return func(e error) error {
		defer sink()
		if e != nil {
			return nil
		}
		return e
	}
}

// Without the defer both halves are reported, so the pair above is measuring
// the recover block and not merely the shape — see bad.go.
