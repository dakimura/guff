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
