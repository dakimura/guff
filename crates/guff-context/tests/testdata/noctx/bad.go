package noctx

import "net/http"

func bad() {
	req, _ := http.NewRequest("GET", "https://example.com", nil)
	_ = req
}
