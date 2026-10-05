// unparam skips a parameter whose type has size zero ("skip - zero size"):
// removing it saves nothing. Sizing a type parameter is impossible, so a type
// that needs one is examined instead (`containsTypeParam`, which since
// 2fa3d841b0c8 sees through aliases and a named type's underlying type).
package zerosize

var DoWork func()

type Empty struct{}

type EmptyAlias = Empty

type Holder[T any] struct{ v T }

type Marker[T any] struct{}

// Silent: every parameter is zero-size.
func zeroSized(a struct{}, b [0]int, c Empty, d [2]struct{}, e struct{ x struct{} }, f EmptyAlias) {
	DoWork()
}

// Reported: not zero-size.
func nonZero(n int, s struct{ x int }) {
	DoWork()
}

// Reported: sizing these needs T.
func withTypeParam[T any](v T, h Holder[T], s struct{ t T }) {
	DoWork()
}

// Silent: Marker[T]'s underlying struct mentions no T, so its size is known: 0.
func markerOnly[T any](m Marker[T]) {
	DoWork()
}
