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
