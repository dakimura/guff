package usestdlibvars

import "net/http"

func Bad() {
	_, _ = http.NewRequest("GET", "http://example.com", nil)
	_ = http.StatusText(404)
}
