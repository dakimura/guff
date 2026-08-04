package p

import "net/http"

func Bad(h http.Header) {
	_ = h.Get("content-type")
}
