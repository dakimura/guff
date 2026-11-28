// Package wrap registers patterns for its callers; it imports net/http itself.
package wrap

import "net/http"

func Mux() *http.ServeMux { return http.NewServeMux() }

func Register(m *http.ServeMux, pat string) {
	m.Handle(pat, nil) // not a constant here
}
