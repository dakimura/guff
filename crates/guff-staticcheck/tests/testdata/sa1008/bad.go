package main

import "net/http"

func main() {
	h := http.Header{}
	_ = h["foo"]
	var r http.Request
	_ = r.Header["etag"]
}
