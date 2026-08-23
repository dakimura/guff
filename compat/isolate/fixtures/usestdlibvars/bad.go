package p

import "net/http"

// usestdlibvars does not flag a string that merely *looks* like a method: it
// walks specific call sites and struct fields. `http.NewRequest`'s first
// argument and a `WriteHeader` argument are two of them; a bare `return "GET"`
// is not, which is why the previous fixture reported nothing.
func Bad() (*http.Request, error) {
	return http.NewRequest("GET", "http://example.com", nil)
}

func BadStatus(w http.ResponseWriter) {
	w.WriteHeader(200)
}
