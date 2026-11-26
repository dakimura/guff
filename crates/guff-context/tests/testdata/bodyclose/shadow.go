// Package shadow isolates one question: does a func literal that declares its
// own `resp` count as capturing the outer one?
//
// Upstream reaches a closure through an `*ssa.MakeClosure` over a *free
// variable*, so a variable the literal declares is not a capture — it is a
// different object that happens to share a name. Four leaks below were silenced
// by matching on the name alone; the two closers must stay silent.
package shadow

import "net/http"

// Reported: the literal declares its own resp and returns it. boundary's
// controller_ratelimit_reload_test.go writes the argument this way.
func literalShadowsAndReturnsIt() {
	fallback, _ := http.NewRequest(http.MethodGet, "https://example.com/f", nil)
	resp, err := http.DefaultClient.Do(func() *http.Request {
		resp, err := http.NewRequest(http.MethodGet, "https://example.com/1", nil)
		if err != nil {
			return fallback
		}
		return resp
	}())
	if err != nil {
		return
	}
	_ = resp.StatusCode
}

// Reported: shadowed, and the literal returns a copy rather than the shadowing
// name — the old suppression fired on the mention, not on the return.
func literalShadowsAndReturnsACopy() {
	fallback, _ := http.NewRequest(http.MethodGet, "https://example.com/f", nil)
	resp, err := http.DefaultClient.Do(func() *http.Request {
		resp, err := http.NewRequest(http.MethodGet, "https://example.com/2", nil)
		if err != nil {
			return fallback
		}
		out := resp
		return out
	}())
	if err != nil {
		return
	}
	_ = resp.StatusCode
}

// Reported: the shadowing literal is unrelated to the call.
func anUnrelatedLiteralShadows() {
	resp, err := http.Get("https://example.com/3")
	if err != nil {
		return
	}
	f := func() *http.Request {
		resp, _ := http.NewRequest(http.MethodGet, "https://example.com/4", nil)
		return resp
	}
	_ = f
	_ = resp.StatusCode
}

// Reported: a plain block is not a func literal, so it never went through this
// path. Here so a fix keying on "declared anywhere inside" cannot pass by
// silencing this one too.
func aBlockShadows() {
	resp, err := http.Get("https://example.com/5")
	if err != nil {
		return
	}
	{
		resp := "not a response"
		_ = resp
	}
	_ = resp.StatusCode
}

// Silent: the literal uses the *outer* resp — a real capture.
func literalCapturesTheOuterResponse() {
	resp, err := http.Get("https://example.com/6")
	if err != nil {
		return
	}
	defer func() { resp.Body.Close() }()
}

// Silent: a shadowing literal first, a capturing one after. The shadow must not
// settle it and the capture must, in that order.
func shadowThenCapture() {
	resp, err := http.Get("https://example.com/7")
	if err != nil {
		return
	}
	_ = func() *http.Request {
		resp, _ := http.NewRequest(http.MethodGet, "https://example.com/8", nil)
		return resp
	}
	defer func() { resp.Body.Close() }()
}
