// The shape upstream is deterministically *silent* on: the only `net/http`
// object named `Header` this package uses is the **method**
// `ResponseWriter.Header`, whose type is `func() Header`, not `Header`.
package canonicalheader

import "net/http"

func methodOnly(w http.ResponseWriter) {
	w.Header().Set("x-request-id", "1")
	w.Header().Add("accept-encoding", "gzip")
	w.Header().Del("user-agent")
	_ = w.Header().Get("content-type")
	_ = w.Header().Values("if-none-match")
}
