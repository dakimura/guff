// The mirror: the only candidate is the **field** `Request.Header`, whose type
// is `http.Header` itself, so every key is checked.
package canonicalheader

import "net/http"

func fieldOnly(r *http.Request) {
	_ = r.Header.Get("content-type")
	_ = r.Header.Values("if-none-match")
}
