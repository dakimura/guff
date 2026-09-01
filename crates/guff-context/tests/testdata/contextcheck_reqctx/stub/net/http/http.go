package http

import "context"

type Request struct{}

// Context is a concrete method on *Request, so calls to it are static in SSA
// (Method == nil) — the shape contextcheck must recognize as an inherited ctx.
func (r *Request) Context() context.Context { return context.Background() }

type ResponseWriter interface {
	Write([]byte) (int, error)
}

type Handler interface {
	ServeHTTP(ResponseWriter, *Request)
}

type HandlerFunc func(ResponseWriter, *Request)

func (f HandlerFunc) ServeHTTP(w ResponseWriter, r *Request) { f(w, r) }
