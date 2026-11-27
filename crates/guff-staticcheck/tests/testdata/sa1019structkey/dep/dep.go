package dep

type Result struct {
	// Deprecated: Use `RequeueAfter` instead.
	Requeue bool
	RequeueAfter int
}

// S2 is an instantiated generic type in a key's literal: `dep.S2[int]{Field2: …}`
// is an *ast.IndexExpr (staticcheck v0.8.2 `SelectorName`). Instantiation
// substitutes only fields whose type mentions a type parameter; Field2 keeps
// its declared object (and its deprecation), GenField becomes a new object
// that the deprecation lookup misses (staticcheck issue 1215).
type S2[T any] struct {
	// Deprecated: Use Live2 instead.
	Field2 string
	// Deprecated: Use Live2 instead.
	GenField T
	Live2    T
	// Deprecated: Composite types without a type parameter are not cloned.
	Slice []string
	// Deprecated: Neither is a function type.
	Hook func(int) error
	// Deprecated: A slice of the type parameter is.
	GenSlice []T
}

// S3 has two type parameters: `dep.S3[int, bool]{F: …}` is an *ast.IndexListExpr.
type S3[T, U any] struct {
	// Deprecated: Use G instead.
	F string
	G U
	H T
}
