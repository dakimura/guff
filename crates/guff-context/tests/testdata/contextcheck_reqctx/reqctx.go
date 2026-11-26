package reqctx

import (
	"context"
	"net/http"
)

func consume(ctx context.Context) { _ = ctx }

// @contextcheck(req_has_ctx)
// TaggedHandler does not have the two-parameter handler shape and never calls
// r.Context(), so only the directive makes it an entry: the fresh context it
// builds is then a finding.
func TaggedHandler(r *http.Request, s string) {
	consume(context.Background())
	_ = s
}

// PlainRequest is the same function without the directive. It is an ordinary
// no-context function, so nothing is reported *here*.
func PlainRequest(r *http.Request, s string) {
	consume(context.Background())
	_ = s
}

// CallsPlainRequest is where PlainRequest's verdict surfaces.
func CallsPlainRequest(ctx context.Context, r *http.Request) {
	consume(ctx)
	PlainRequest(r, "x")
}

// CanonicalHandler has the two-parameter shape, which needs no directive.
func CanonicalHandler(w http.ResponseWriter, r *http.Request) {
	consume(context.Background())
	_ = w
}
