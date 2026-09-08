// go-ethereum v1.17.5 pins x/exp at v0.0.0-20230626212559, which predates the
// //go:fix directives, and does not vendor. Before the module-cache lookup the
// hardcoded table decided here and guff reported a finding upstream does not.
package maps

func Clone[M ~map[K]V, K comparable, V any](m M) M { return m }
