package main

import "net/http"

func main() {
	var h http.Handler
	http.ListenAndServe("localhost:8080/", h)
	http.ListenAndServe("localhost", h)
	http.ListenAndServeTLS("bad:99999", "", "", h)
}
