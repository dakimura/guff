package freevar

// A response stored into a variable the closure does not own goes through an
// `ssa.FreeVar`. What settles it is `isClosureCalled` + `calledInFunc` over a
// *nested* literal that captures the same variable: a `MakeClosure` with a
// `Call` or `Defer` referrer — invoked, deferred, or passed as an argument —
// answers "not open" at the closure's first instruction that is not a load,
// whether or not that closure closes anything. datadog-agent's
// `pkg/util/ecs/metadata/v3or4/client.go` defers
// `func() { telemetry.AddQueryToTelemetry(path, resp) }` around its
// `resp, err = client.Do(req)`.
//
// A literal that is never invoked settles nothing, `go func(){…}()` is neither
// a Call nor a Defer, and `_ = resp` loads nothing at all — go/ssa drops the
// assignment, so that literal captures no free variable.
//
// Measured against golangci-lint 2.12.2 (bodyclose v0.1.0).

import "net/http"

func retry(f func() error) error { return f() }

// A: datadog — outer `var resp`, assigned and closed inside a closure that is
// passed to another function
func A(c *http.Client, req *http.Request) error {
	var resp *http.Response
	operation := func() error {
		var err error
		resp, err = c.Do(req)
		defer func() { _ = resp }()
		if err != nil {
			return err
		}
		defer resp.Body.Close()
		return nil
	}
	return retry(operation)
}

// B: same, but the closure is called directly
func B(c *http.Client, req *http.Request) error {
	var resp *http.Response
	operation := func() error {
		var err error
		resp, err = c.Do(req)
		if err != nil {
			return err
		}
		defer resp.Body.Close()
		return nil
	}
	return operation()
}

// C: same, closure never called
func C(c *http.Client, req *http.Request) func() error {
	var resp *http.Response
	return func() error {
		var err error
		resp, err = c.Do(req)
		if err != nil {
			return err
		}
		defer resp.Body.Close()
		return nil
	}
}

// D: outer var, assigned in closure, NOT closed anywhere
func D(c *http.Client, req *http.Request) error {
	var resp *http.Response
	operation := func() error {
		var err error
		resp, err = c.Do(req)
		_ = resp
		return err
	}
	return retry(operation)
}

func sink(r *http.Response) {}

// E: datadog exactly — a deferred closure that *uses* resp, then the close
func E(c *http.Client, req *http.Request) error {
	var resp *http.Response
	operation := func() error {
		var err error
		resp, err = c.Do(req)
		defer func() { sink(resp) }()
		if err != nil {
			return err
		}
		defer resp.Body.Close()
		return nil
	}
	return retry(operation)
}

// F: the same deferred closure, but nothing closes the body
func F(c *http.Client, req *http.Request) error {
	var resp *http.Response
	operation := func() error {
		var err error
		resp, err = c.Do(req)
		defer func() { sink(resp) }()
		return err
	}
	return retry(operation)
}

// G: the capturing closure is called immediately instead of deferred
func G(c *http.Client, req *http.Request) error {
	var resp *http.Response
	operation := func() error {
		var err error
		resp, err = c.Do(req)
		func() { sink(resp) }()
		return err
	}
	return retry(operation)
}

// H: the capturing closure is never called
func H(c *http.Client, req *http.Request) error {
	var resp *http.Response
	operation := func() error {
		var err error
		resp, err = c.Do(req)
		_ = func() { sink(resp) }
		return err
	}
	return retry(operation)
}
