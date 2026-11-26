package p

import "net/http"

func Bad(h http.Header) {
	_ = h.Get("content-type")
}

// The message quotes the header it saw and the canonical spelling, so every
// header name is a different sentence. `Set`, `Add` and `Del` are separate call
// sites upstream matches, not just `Get`.
func More(h http.Header) {
	h.Set("x-request-id", "1")
	h.Add("accept-encoding", "gzip")
	h.Del("user-agent")
	_ = h.Values("if-none-match")
}

// Negatives: a header whose MIME-canonical form is in upstream's initialism
// table is **not** reported at all — the table suppresses rather than
// suggests. guff used to report these with the mapped spelling.
func WellKnownAreSilent(h http.Header) {
	_ = h.Get("etag")
	_ = h.Get("www-authenticate")
	_ = h.Get("x-request-id")
}
