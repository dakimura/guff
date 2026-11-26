package main

import "net/http"

func main() {
	var h http.Handler
	http.ListenAndServe("localhost:8080", h)
	http.ListenAndServe(":8080", h)
	http.ListenAndServe(":http", h)
	http.ListenAndServe("localhost:http", h)
	http.ListenAndServe("local_host:8080", h)
	http.ListenAndServe("", h)
}
