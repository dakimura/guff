package main

import "net/http"

func f(h http.Header) { h.Set(http.CanonicalHeaderKey("X"), "v") }
