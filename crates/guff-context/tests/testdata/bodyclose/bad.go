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
