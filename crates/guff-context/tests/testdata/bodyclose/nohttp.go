// Package nohttp reaches *http.Response only through a dependency: it never
// imports net/http itself. Upstream's bodyclose looks the response type up in
// `pass.Pkg.Imports()` — direct imports, no transitivity — and returns without
// checking anything when it is not there, so neither call below is reported.
// scaleway-cli's internal/gotty is this shape, dialing with gorilla/websocket.
package nohttp

import "example.com/bodyclose/dialer"

func dropped(u string) error {
	conn, _, err := dialer.Dial(u)
	if err != nil {
		return err
	}
	_ = conn

	return nil
}

func neverClosed(u string) error {
	resp, err := dialer.Fetch(u)
	if err != nil {
		return err
	}
	_ = resp

	return nil
}
