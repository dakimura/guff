package bodyclose

import "net/http"

func missingClose() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	_ = resp.StatusCode
}

func discarded() {
	http.Get("https://example.com")
}

func missingAfterReassign() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	_ = resp.Body.Close()
	resp, err = http.Get("https://example.com/2")
	if err != nil {
		return
	}
	_ = resp.Status
}

// The response goes to the blank identifier, so there is no `ssa.Extract` for
// `isopen` to follow and upstream falls through to its default: reported.
// dapr's `tests/integration/suite/healthz` wants only the error.
func blankResponse() {
	_, err := http.Get("https://example.com")
	if err != nil {
		return
	}
}

func blankBoth() {
	_, _ = http.Get("https://example.com")
}

// `getReqCall` is a substring test over the printed call type, so a call
// returning a *function* that returns a response qualifies — and `getResVal`
// then finds no response value to follow, so the walk reports. cli's
// `httpmock.ScopesResponder` is this shape.
func statusResponder(code int) func(*http.Request) (*http.Response, error) {
	return func(req *http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: code}, nil
	}
}

func scopesResponder() func(*http.Request) (*http.Response, error) {
	return statusResponder(http.StatusOK)
}

func responseChan() chan *http.Response { return nil }

func useResponseChan() {
	ch := responseChan()
	_ = ch
}

// A response stored into a variable the *enclosing* function owns goes through
// an `ssa.FreeVar`, whose referrers live inside the closure: there is no
// `MakeClosure` to follow and no field store, so nothing proves the body is
// closed — the `resp.Body.Close()` below reads the variable, not the call's
// result. dapr silences four of these in
// `tests/integration/suite/actors/http/ttl.go`.
func closureReassigns(run func(func())) {
	resp, err := http.Get("https://example.com/1")
	if err != nil {
		return
	}
	_ = resp.Body.Close()
	run(func() {
		req, rerr := http.NewRequest("GET", "https://example.com/2", nil)
		if rerr != nil {
			return
		}
		_ = req
		resp, err = http.Get("https://example.com/2") // want
		if err != nil {
			return
		}
		_ = resp.Body.Close()
	})
}

// When the closure *opens* as its first statement, `calledInFunc` answers
// `isopen(b, i) || !called` there, and the assignment outside is reported too.
func closureOpensFirst(run func(func())) {
	resp, err := http.Get("https://example.com/1") // want
	if err != nil {
		return
	}
	_ = resp.Body.Close()
	run(func() {
		resp, err = http.Get("https://example.com/2") // want
		if err != nil {
			return
		}
		_ = resp.Body.Close()
	})
}

// Two assignments to one variable are two SSA values, and the second only
// kills the first when it *dominates* it. Upstream's
// `len(*call.Referrers()) == 0 { return true }` is what reports a killed
// value; a merged one reaches a `*ssa.Phi` instead, and the phi arm of
// `isopen` settles every value that flows into it.
//
// Every shape below has at least one value that really is killed, or one the
// close cannot reach. The silent halves live in `ok.go`.

// The second store is in the same block, so the first is dead.
func sequentialReassign() {
	resp, err := http.Get("https://example.com/seq-1") // want
	if err != nil {
		return
	}
	resp, err = http.Get("https://example.com/seq-2")
	if err != nil {
		return
	}
	defer resp.Body.Close()
}

// One arm assigns, then an unconditional store kills it.
func armThenUnconditional(x bool) {
	var resp *http.Response
	var err error
	if x {
		resp, err = http.Get("https://example.com/arm") // want
	}
	resp, err = http.Get("https://example.com/after")
	if err != nil {
		return
	}
	defer resp.Body.Close()
}

// Twice in the *same* arm: the second kills the first there too.
func sameArmTwice(x bool) {
	var resp *http.Response
	var err error
	if x {
		resp, err = http.Get("https://example.com/same-1") // want
		resp, err = http.Get("https://example.com/same-2")
	}
	if err != nil {
		return
	}
	if resp != nil {
		resp.Body.Close()
	}
}

// A loop body is a block like any other.
func loopTwoAssigns() {
	var resp *http.Response
	var err error
	for i := 0; i < 3; i++ {
		resp, err = http.Get("https://example.com/loop-1") // want
		resp, err = http.Get("https://example.com/loop-2")
	}
	if err != nil {
		return
	}
	if resp != nil {
		resp.Body.Close()
	}
}

// Merged, but the close is outside the loop the merge happened in, so the
// value passes through the loop header's phi as well — and `isopen` does not
// follow a phi into another phi. Both are reported.
func loopArmAssignClosedAfter(x bool) {
	var resp *http.Response
	var err error
	for i := 0; i < 3; i++ {
		if x {
			resp, err = http.Get("https://example.com/loop-arm-1") // want
		} else {
			resp, err = http.Get("https://example.com/loop-arm-2") // want
		}
	}
	if err != nil {
		return
	}
	if resp != nil {
		resp.Body.Close()
	}
}

// The same one loop further in: merged at depth 2, closed at depth 1.
func innerLoopArmClosedInOuter(x bool) {
	var resp *http.Response
	var err error
	for i := 0; i < 3; i++ {
		for j := 0; j < 3; j++ {
			if x {
				resp, err = http.Get("https://example.com/two-loops-1") // want
			} else {
				resp, err = http.Get("https://example.com/two-loops-2") // want
			}
		}
		if err != nil {
			return
		}
		resp.Body.Close()
	}
}

// Merged, and never closed at all.
func unconditionalThenArmNotClosed(x bool) {
	resp, err := http.Get("https://example.com/merge-open-1") // want
	if err != nil {
		return
	}
	if x {
		resp, err = http.Get("https://example.com/merge-open-2") // want
		if err != nil {
			return
		}
	}
	_ = resp
}

// Merged, and only one arm closes: the phi has no `FieldAddr` of its own, so
// the arm that does not close is reported.
func oneArmCloses(x bool) {
	var resp *http.Response
	var err error
	if x {
		resp, err = http.Get("https://example.com/one-arm-1")
		defer resp.Body.Close()
	} else {
		resp, err = http.Get("https://example.com/one-arm-2") // want
	}
	if err != nil {
		return
	}
}

// A func literal that declares its *own* `resp` is not a capture of this one:
// upstream reaches a closure through an `*ssa.MakeClosure` over a free
// variable, and a variable the literal declares is not free. boundary's
// `controller_ratelimit_reload_test.go` writes the argument this way sixteen
// times, and every one went unreported.
func literalShadowsTheName() {
	req, _ := http.NewRequest(http.MethodGet, "https://example.com/shadow-0", nil)
	resp, err := http.DefaultClient.Do(func() *http.Request { // want
		resp, err := http.NewRequest(http.MethodGet, "https://example.com/shadow-1", nil)
		if err != nil {
			return req
		}
		return resp
	}())
	if err != nil {
		return
	}
	_ = resp.StatusCode
}

// The same shadow, returning a copy rather than the shadowing name — the
// suppression fired on *mentioning* the name, so this went unreported too.
func literalShadowsAndReturnsACopy() {
	req, _ := http.NewRequest(http.MethodGet, "https://example.com/shadow-2", nil)
	resp, err := http.DefaultClient.Do(func() *http.Request { // want
		resp, err := http.NewRequest(http.MethodGet, "https://example.com/shadow-3", nil)
		if err != nil {
			return req
		}
		out := resp
		return out
	}())
	if err != nil {
		return
	}
	_ = resp.StatusCode
}

// The shadowing literal need not be the call's argument, or related to the call
// at all: any literal later in the same function was enough.
func anUnrelatedLiteralShadowsTheName() {
	resp, err := http.Get("https://example.com/shadow-4") // want
	if err != nil {
		return
	}
	f := func() *http.Request {
		resp, _ := http.NewRequest(http.MethodGet, "https://example.com/shadow-5", nil)
		return resp
	}
	_ = f
	_ = resp.StatusCode
}

// A plain block that shadows the name is not a func literal and was always
// reported. Here so that a fix keying on "declared anywhere inside" cannot pass
// by silencing this one as well.
func aBlockShadowsTheName() {
	resp, err := http.Get("https://example.com/shadow-6") // want
	if err != nil {
		return
	}
	{
		resp := "not a response"
		_ = resp
	}
	_ = resp.StatusCode
}
