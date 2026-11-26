// Package inplace covers two ways a `CallExpr` reached the report that upstream
// never offers to its own test.
//
// Upstream matches `*ssa.Call` instructions. A conversion is not one — it
// lowers to `ChangeType` / `Convert` / `MakeInterface` — and a call whose
// result has its `Body` closed in the same expression is answered by the
// referrer walk, which finds the `FieldAddr` for `Body` and the `Close` on it.
package inplace

import "net/http"

type wrapper struct{ r *http.Response }

func (w *wrapper) HttpResponse() *http.Response { return w.r }

func get() *wrapper { return nil }

// Silent: closed in the very expression the call appears in. `render_chain`
// stops at a call, so `body_close_chain` recorded nothing here and the call it
// closes was reported.
func closedInPlace() {
	get().HttpResponse().Body.Close()
}

// Reported: a second call is a second value and this one is never closed.
// boundary's internal/clientcache writes both lines together, and upstream
// reports only the second.
func closedThenReadAgain() int {
	w := get()
	w.HttpResponse().Body.Close()
	return w.HttpResponse().StatusCode
}

// Silent: a conversion. Its printed type contains `*net/http.Response`, which
// is exactly why upstream's substring test also excludes `ResponseController`
// by name — the test is faithful, feeding it AST calls is not.
func responseWriterConversion() any {
	return (*http.ResponseWriter)(nil)
}

// Silent for the same reason, with a type that really is a response — so a fix
// keyed on the *name* rather than on "this is a conversion" would still report
// here.
func responseConversion() any {
	return (*http.Response)(nil)
}

// Reported: an ordinary leak, so the file is not silent by accident.
func plainLeak() int {
	resp, _ := http.Get("https://example.com/inplace")
	return resp.StatusCode
}
