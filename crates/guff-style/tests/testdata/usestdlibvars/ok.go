package usestdlibvars

import "net/http"

func Ok() {
	_, _ = http.NewRequest(http.MethodGet, "http://example.com", nil)
	_ = http.StatusText(http.StatusNotFound)
}
