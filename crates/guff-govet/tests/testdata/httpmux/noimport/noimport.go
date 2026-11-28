// Package noimport reaches *http.ServeMux only through wrap, so it does not
// import net/http directly and httpmux skips it entirely.
package noimport

import "example.com/httpmux/wrap"

func _() {
	wrap.Mux().Handle("GET /skipped", nil)
	wrap.Mux().HandleFunc("/{skipped}", nil)
	wrap.Register(wrap.Mux(), "GET /also-skipped")
}
