// x/exp from 2025-02-10 on, where the directive is really there. consul and
// vault are on this side and match upstream on nine findings.
package maps

//go:fix inline
func Clone[M ~map[K]V, K comparable, V any](m M) M { return m }
