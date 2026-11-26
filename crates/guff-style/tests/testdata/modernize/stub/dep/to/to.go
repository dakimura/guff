// Package to stands in for a wrapper declared in another module — azcore's
// `to` is the one beats uses. What matters is that guff does *not* analyse it,
// so no `newLike` fact is exported for `Ptr` and the call site has to decide
// from this file's source.
package to

// Ptr is new-like: one parameter, one pointer result, body `return &v`.
func Ptr[T any](v T) *T {
	return &v
}

// NotNewLike copies first, so the address is not the parameter's.
func NotNewLike[T any](v T) *T {
	w := v
	return &w
}

// TwoParams has the right body but the wrong arity.
func TwoParams[T any](v, unused T) *T {
	return &v
}

var shared int

// Shared returns the address of a package-level variable.
func Shared(v int) *int {
	return &shared
}
