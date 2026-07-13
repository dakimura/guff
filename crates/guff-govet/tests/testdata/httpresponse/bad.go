package p

import "net/http"

func f() {
	resp, err := http.Get("http://example.com")
	defer resp.Body.Close()
	_ = err
}
