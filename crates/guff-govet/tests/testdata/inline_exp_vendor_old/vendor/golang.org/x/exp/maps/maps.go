// The x/exp that lazygit v0.64.1 vendors (v0.0.0-20240719175910) predates the
// //go:fix directives, so Clone carries none and upstream says nothing at a
// call site.
package maps

func Clone[M ~map[K]V, K comparable, V any](m M) M { return m }
