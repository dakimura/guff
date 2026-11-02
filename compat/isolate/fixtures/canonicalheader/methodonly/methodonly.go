// Package methodonly: the only `net/http` object it uses is the method
// `ResponseWriter.Header`.
//
// Until golangci-lint 2.15.0 canonicalheader opened by picking one `net/http`
// object named `Header` from `pass.TypesInfo.Uses` (whichever the map handed it
// first) and required every receiver to be identical to that object's type, so
// a package whose only candidate was the method — type `func() Header` —
// reported nothing. 2.15.0's fork checks the receiver itself
// (`isHTTPHeader`), so these keys are reported like any other.
package methodonly

import "net/http"

func Silent(w http.ResponseWriter) {
	w.Header().Set("x-request-id", "1")
	w.Header().Add("accept-encoding", "gzip")
	w.Header().Del("user-agent")
	_ = w.Header().Get("content-type")
	_ = w.Header().Values("if-none-match")
}
