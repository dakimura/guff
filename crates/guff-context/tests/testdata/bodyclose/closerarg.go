// Package closerarg covers bodyclose's `isCloseCall` `*ssa.ChangeInterface`
// arm.
//
// `defer closing(resp.Body, tag)` converts the body to `io.Closer` — an
// `*ssa.ChangeInterface` — and upstream then walks *that* value's referrers for
// a `*ssa.Defer` whose callee contains a call to `(io.Closer).Close`. beats
// writes it in `libbeat/esleg/eslegclient/connection.go:512`.
//
// A plain, non-deferred call never reaches the arm: the referrer is an
// `*ssa.Call`, not a `*ssa.Defer`. Every mark below was measured against
// golangci-lint 2.12.2, and `compat/golden/cases/bodyclose` runs both tools
// over this file.
package closerarg

import (
	"io"
	"net/http"
)

func closing(c io.Closer)                 { _ = c.Close() }
func closingTagged(c io.Closer, s string) { _ = c.Close() }
func closingReadCloser(c io.ReadCloser)   { _ = c.Close() }
func closingAny(c any)                    {}
func notClosing(c io.Closer)              { _ = c }

type helper struct{}

func (helper) closeIt(c io.Closer) { _ = c.Close() }

// silent: the conversion happens and the callee closes.
func deferred(c *http.Client, req *http.Request) error {
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	defer closing(resp.Body)
	return nil
}

// silent: the body is one of two arguments — the beats shape.
func deferredTagged(c *http.Client, req *http.Request) error {
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	defer closingTagged(resp.Body, "tag")
	return nil
}

// silent: a method is a static callee too.
func deferredMethod(c *http.Client, req *http.Request, h helper) error {
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	defer h.closeIt(resp.Body)
	return nil
}

// FINDING: not deferred, so the referrer is a Call and the arm never fires.
func direct(c *http.Client, req *http.Request) error {
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	closing(resp.Body)
	return nil
}

// FINDING: the parameter is `io.ReadCloser`, so there is no conversion.
func readCloserParam(c *http.Client, req *http.Request) error {
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	defer closingReadCloser(resp.Body)
	return nil
}

// FINDING: the parameter is `any`.
func anyParam(c *http.Client, req *http.Request) error {
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	defer closingAny(resp.Body)
	return nil
}

// FINDING: the callee takes an `io.Closer` and does not close it.
func notClosingCallee(c *http.Client, req *http.Request) error {
	resp, err := c.Do(req)
	if err != nil {
		return err
	}
	defer notClosing(resp.Body)
	return nil
}
