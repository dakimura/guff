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
