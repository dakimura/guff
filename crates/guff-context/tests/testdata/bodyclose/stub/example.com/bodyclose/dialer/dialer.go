// Package dialer names *http.Response in its own signatures. A caller that
// imports only this package never mentions net/http, which is what upstream's
// `LookupFromImports(pass.Pkg.Imports(), "net/http", "Response")` looks for.
package dialer

import "net/http"

type Conn struct{}

// Dial has gorilla/websocket's shape: the response is the second result and
// callers routinely drop it.
func Dial(url string) (*Conn, *http.Response, error) {
	return &Conn{}, nil, nil
}

func Fetch(url string) (*http.Response, error) {
	return nil, nil
}
