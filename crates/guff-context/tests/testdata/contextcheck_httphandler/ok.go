package contextcheck_httphandler

import (
	"context"
	"net/http"
)

func work(ctx context.Context) { _ = ctx }

// Plain handler: ctx comes from r.Context(), so nothing is non-inherited.
func Handler(w http.ResponseWriter, r *http.Request) {
	ctx := r.Context()
	work(ctx)
}

// Same, but the handler is a closure returned by a function that has its own
// ctx parameter — the shape used by http middleware adapters.
func Adapter(ctx context.Context) http.Handler {
	_ = ctx
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		inner := r.Context()
		work(inner)
	})
}

// r.Context() passed straight through without a local binding.
func Direct(w http.ResponseWriter, r *http.Request) {
	work(r.Context())
}
