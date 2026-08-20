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

