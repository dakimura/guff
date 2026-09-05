package bodyclose

import (
	"io"
	"net/http"
	"net/http/httptest"
)

func withClose() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer resp.Body.Close()
	_ = resp.StatusCode
}

func withDeferFunc() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer func() { _ = resp.Body.Close() }()
	_ = resp.StatusCode
}

func withConsumeAndClose() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer resp.Body.Close()
	_, _ = io.ReadAll(resp.Body)
}

func returnsResponse() (*http.Response, error) {
	return http.Get("https://example.com")
}

func returnsResponseNamed() (*http.Response, error) {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return nil, err
	}
	return resp, nil
}

func httptestResult() {
	rec := &httptest.ResponseRecorder{}
	resp := rec.Result()
	_ = resp.StatusCode
}

func withCleanup(t interface{ Cleanup(func()) }) {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	t.Cleanup(func() {
		_ = resp.Body.Close()
	})
	_ = resp.StatusCode
}

func passToHelper() error {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return err
	}
	return handleResponse(resp)
}

func handleResponse(resp *http.Response) error {
	defer resp.Body.Close()
	_, err := io.ReadAll(resp.Body)
	return err
}

func syntheticResponse() {
	res := &http.Response{
		StatusCode: 200,
	}
	_ = res.StatusCode
}

// `make` and `new` are not calls in go/ssa — they lower to `MakeChan`,
// `MakeMap`, `MakeSlice` and `Alloc` — so the substring test in `getReqCall`,
// which only sees `*ssa.Call`, never reaches them. dapr passes responses over
// exactly this channel in `daprd/shutdown/graceful`.
func makeResponseChannel() {
	respCh := make(chan *http.Response)
	_ = respCh
	resp := new(http.Response)
	_ = resp
}

// A named function type does not print its underlying signature, so the
// substring is not there either.
type responder func(*http.Request) (*http.Response, error)

func namedResponder() responder { return nil }

func useNamedResponder() responder {
	return namedResponder()
}

// A *nested* closure that closes the body is what upstream follows out of the
// free variable: the `MakeClosure` among its referrers leads to
// `calledInFunc`, which finds the `Close` on the `io.ReadCloser` and answers
// "not open". dapr's outbox tests close inside `t.Cleanup(func(){…})`.
func closureClosesInNestedClosure(run func(func()), cleanup func(func())) {
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
		resp, err = http.Get("https://example.com/2")
		if err != nil {
			return
		}
		cleanup(func() {
			_ = resp.Body.Close()
		})
	})
}

// suite stands in for Ginkgo's Describe: a function taking a closure, called
// from a package-level variable initializer.
func suite(name string, body func()) bool { return true }

// Everything below is unreachable for upstream bodyclose, which walks
// go/analysis's `SrcFuncs` — built from a file's *ast.FuncDecl*s and their
// nested literals, and from nothing else. A literal inside a package-level
// `var` initializer belongs to the synthesized package `init`, which has no
// FuncDecl, so no SSA-based analyzer ever reaches it.
//
// This is how a whole Ginkgo test file disappears: those are written as
// `var _ = Describe("…", func() { … })`.
var _ = suite("unclosed in a var initializer", func() {
	resp, err := http.Get("https://example.com/varinit")
	if err != nil {
		return
	}
	_ = resp.StatusCode
})

var _ = suite("discarded in a var initializer", func() {
	_, err := http.Get("https://example.com/varinit-blank")
	_ = err
})

// The same through a nested literal, one level deeper.
var _ = suite("nested", func() {
	suite("inner", func() {
		resp, err := http.Get("https://example.com/varinit-nested")
		if err != nil {
			return
		}
		_ = resp.StatusCode
	})
})

// A `const` block cannot hold a closure, but a package-level var with an
// explicit type can, and so can a grouped `var (...)`.
var (
	_ = suite("grouped", func() {
		resp, _ := http.Get("https://example.com/varinit-grouped")
		_ = resp
	})
)

// The merge shapes that stay silent — the counterparts of the ones in
// `bad.go`. Two stores to one variable that do not kill each other reach a
// `*ssa.Phi`, and `isopen`'s phi arm settles every value behind it when the
// body is closed through that phi.

// telegraf's `plugins/inputs/prometheus/prometheus.go`: two clients, one
// `resp`, one deferred close.
func twoArmsOneClose(x bool) {
	var resp *http.Response
	var err error
	if x {
		resp, err = http.Get("https://example.com/two-arms-1")
	} else {
		resp, err = http.Get("https://example.com/two-arms-2")
	}
	if err != nil {
		return
	}
	defer resp.Body.Close()
}

// Three arms of a `switch` are still one phi.
func threeArmsOneClose(x int) {
	var resp *http.Response
	var err error
	switch x {
	case 0:
		resp, err = http.Get("https://example.com/three-1")
	case 1:
		resp, err = http.Get("https://example.com/three-2")
	default:
		resp, err = http.Get("https://example.com/three-3")
	}
	if err != nil {
		return
	}
	defer resp.Body.Close()
}

// And so is a `select`.
func selectArmsOneClose(ch chan int) {
	var resp *http.Response
	var err error
	select {
	case <-ch:
		resp, err = http.Get("https://example.com/select-1")
	default:
		resp, err = http.Get("https://example.com/select-2")
	}
	if err != nil {
		return
	}
	defer resp.Body.Close()
}

// The unconditional store comes *first*: it survives on the branch's other
// edge, so it merges rather than dying. (Reverse the order and the second
// store kills the first — `armThenUnconditional` in `bad.go`.)
func unconditionalThenArm(x bool) {
	resp, err := http.Get("https://example.com/merge-1")
	if err != nil {
		return
	}
	if x {
		resp, err = http.Get("https://example.com/merge-2")
		if err != nil {
			return
		}
	}
	defer resp.Body.Close()
}

// Nested branches merge at the inner one first.
func nestedArmsOneClose(x, y bool) {
	var resp *http.Response
	var err error
	if x {
		if y {
			resp, err = http.Get("https://example.com/nested-1")
		} else {
			resp, err = http.Get("https://example.com/nested-2")
		}
	} else {
		resp, err = http.Get("https://example.com/nested-3")
	}
	if err != nil {
		return
	}
	defer resp.Body.Close()
}

// Merged inside a loop and closed inside the same loop: one phi, so the close
// reaches it. (Close it after the loop instead and there are two —
// `loopArmAssignClosedAfter` in `bad.go`.)
func branchInLoopClosedInLoop(x bool) {
	var resp *http.Response
	var err error
	for i := 0; i < 3; i++ {
		if x {
			resp, err = http.Get("https://example.com/in-loop-1")
		} else {
			resp, err = http.Get("https://example.com/in-loop-2")
		}
		if err != nil {
			return
		}
		resp.Body.Close()
	}
}

// Each arm closes its own response before the next store.
func bothArmsClose(x bool) {
	var resp *http.Response
	var err error
	if x {
		resp, err = http.Get("https://example.com/both-1")
		if err != nil {
			return
		}
		resp.Body.Close()
	} else {
		resp, err = http.Get("https://example.com/both-2")
		if err != nil {
			return
		}
		resp.Body.Close()
	}
}
