// consul and vault do not vendor either, but are on 2025-05 / 2025-08 x/exp,
// where the directive is really there. The nine findings they match upstream on
// have to survive the module-cache lookup.
package maps

//go:fix inline
func Clone[M ~map[K]V, K comparable, V any](m M) M { return m }
