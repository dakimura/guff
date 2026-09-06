// Package wrap owns a type *named* Response that is not net/http's. Upstream
// skips a function whole when one of its results is `*net/http.Response`, and
// it decides that on the resolved type — so a result of this package's
// `*wrap.Response` skips nothing.
package wrap

import "net/http"

// Response has net/http's name and none of its identity.
type Response struct {
	R *http.Response
}

// Wrapper is the same type under a name that could never be confused for it.
type Wrapper struct {
	R *http.Response
}

func New(r *http.Response) *Response { return &Response{R: r} }

func NewWrapper(r *http.Response) *Wrapper { return &Wrapper{R: r} }
