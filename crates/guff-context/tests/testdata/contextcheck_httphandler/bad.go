package contextcheck_httphandler

import (
	"context"
	"net/http"
)

func consume(ctx context.Context) { _ = ctx }

// A handler that builds a fresh root context instead of using r.Context() is
// still a finding — the fix must not blanket-exempt http handlers.
func BadHandler(w http.ResponseWriter, r *http.Request) {
	consume(context.Background())
}
