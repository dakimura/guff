package main

import "net/http"

const hostHeader = "host"

func appendSelf(h http.Header, v string) {
	// An assignment that writes an `http.Header` key takes its whole subtree
	// out of the walk — upstream's `return false` — so the read on the right is
	// not reported even though the key is not canonical. Its own TODO says so:
	// "this risks missing some Header reads, for example in
	// `h1["foo"] = h2["foo"]`". k6
	// `internal/output/prometheusrw/sigv4/sigv4.go` writes two of these.
	h[hostHeader] = append(h[hostHeader], v)
}

func appendSelfLiteral(h http.Header, v string) {
	h["host"] = append(h["host"], v)
}

func appendFromOther(h, other http.Header) {
	// Both indexes on the right are skipped, not just the one that matches the
	// key being written.
	h[hostHeader] = append(h[hostHeader], other["content-length"]...)
}

func literalInsideSkipped(h http.Header) {
	// Including one inside a function literal.
	h[hostHeader] = func() []string {
		return h["content-length"]
	}()
}

func main() {
	h := http.Header{}
	_ = h["Foo"]
	var s []string
	h["Foo"] = s
	var m map[string][]string
	_ = m["foo"]
	appendSelf(h, "x")
	appendSelfLiteral(h, "x")
	appendFromOther(h, h)
	literalInsideSkipped(h)
}
