// Package fieldonly is the mirror of `methodonly`: the only `net/http` object
// named `Header` in this package is the **field** `Request.Header`, whose type
// is `http.Header` itself, so the identity test holds and every key is checked.
//
// The two packages together pin both deterministic halves. A package that has
// *both* — syncthing's `lib/api` does — is a coin flip upstream, since the
// candidate is chosen by map iteration order; see docs/COMPAT-HARDENING.md.
package fieldonly

import "net/http"

func Reported(r *http.Request) {
	_ = r.Header.Get("content-type")
	_ = r.Header.Values("if-none-match")
}
