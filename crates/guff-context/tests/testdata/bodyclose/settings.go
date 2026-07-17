package bodyclose

import (
	"io"
	"net/http"
)

// Closed but not consumed — flagged only when check-consumption is true.
func closedOnly() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer resp.Body.Close()
	_ = resp.StatusCode
}

func closedAndConsumed() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer resp.Body.Close()
	_, _ = io.ReadAll(resp.Body)
}
