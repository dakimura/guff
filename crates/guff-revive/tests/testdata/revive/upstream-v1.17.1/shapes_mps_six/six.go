// Not upstream: six exported struct types once the non-ASCII name, the alias,
// the generic type and the function-local type are counted.
package six

type A struct{}
type Éclair struct{}
type Al = struct{}
type G[T any] struct{ v T }

func f() {
	type Local struct{}
	_ = Local{}
}

type Ω struct{}
