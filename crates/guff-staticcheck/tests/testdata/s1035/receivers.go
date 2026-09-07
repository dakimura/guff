package main

import "net/http"

// Which method the call actually resolves to.
//
// Upstream's pattern matches the callee *object*
// (`(Symbol "(net/http.Header).Set")`), not the selector's spelling. guff
// matched on the name `Set`/`Add`/`Get`/`Del` alone and printed the receiver
// expression's own type, which went wrong in both directions:
//
//   * a type that embeds `http.Header` and **shadows** `Set` with its own
//     method was reported, though the callee is not `net/http.Header`'s — that
//     is minio's `cmd/postpolicyform_test.go`, seven findings;
//   * a type with a `Set` method and no relation to `net/http` was reported too;
//   * and for a **promoted** method, where the callee really is
//     `net/http.Header`'s, the message named the outer type.

// Embeds http.Header and shadows Set.
type formValues struct {
	http.Header
}

func (f formValues) Set(key, value string) formValues {
	f.Header.Set(key, value)
	return f
}

// Embeds http.Header and shadows nothing: Del is promoted.
type wrapper struct {
	http.Header
}

// Unrelated to net/http entirely.
type unrelated struct{}

func (unrelated) Set(key, value string) {}

// silent: the callee is (main.formValues).Set
func shadowed(f formValues) formValues {
	return f.Set(http.CanonicalHeaderKey("X-Foo"), "v")
}

// reported as (net/http.Header).Del: the callee is the promoted method
func promoted(w wrapper) {
	w.Del(http.CanonicalHeaderKey("X-Foo"))
}

// reported, all four
func direct(h http.Header) {
	h.Set(http.CanonicalHeaderKey("X-Foo"), "v")
	h.Add(http.CanonicalHeaderKey("X-Foo"), "v")
	_ = h.Get(http.CanonicalHeaderKey("X-Foo"))
	h.Del(http.CanonicalHeaderKey("X-Foo"))
}

// silent: nothing to do with net/http
func unrelatedSet(u unrelated) {
	u.Set(http.CanonicalHeaderKey("X-Foo"), "v")
}
