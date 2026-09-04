// Package methodonly is the shape upstream is deterministically *silent* on.
//
// canonicalheader opens by scanning `pass.TypesInfo.Uses` for any `net/http`
// object named `Header` and keeping whichever one the map hands it first, then
// requires `types.Identical(recv, headerObject.Type())` at every call site.
// `net/http` has four such objects: the type `Header`, the fields
// `Request.Header` and `Response.Header` — whose type *is* `http.Header` — and
// the method `ResponseWriter.Header`, whose type is `func() Header`.
//
// Here the method is the only candidate, so the identity test never holds and
// the package reports nothing, however non-canonical the keys are.
package methodonly

import "net/http"

func Silent(w http.ResponseWriter) {
	w.Header().Set("x-request-id", "1")
	w.Header().Add("accept-encoding", "gzip")
	w.Header().Del("user-agent")
	_ = w.Header().Get("content-type")
	_ = w.Header().Values("if-none-match")
}
