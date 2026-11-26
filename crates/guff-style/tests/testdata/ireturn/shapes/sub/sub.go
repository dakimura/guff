package sub

// Iface lives in a package of a module whose path has no dot (`ireturnshapes`).
// A "first path element has no `.`" rule calls that standard library.
type Iface interface{ M() }

type Gen[T any] interface{ Get() T }
