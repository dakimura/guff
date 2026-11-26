package p

// varnamelen has five kinds, each with its own message and its own position:
// `variable` and `constant` come off the assignment or the ValueSpec,
// `parameter` / `return value` / `type parameter` off the Field. One short
// variable exercises one of the five.
//
// It measures the *distance* a name travels, not its length alone:
// `min-name-length` is 3 but a name used within `max-distance` (5) lines of its
// declaration is fine, which is why `i := 1; _ = i` reports nothing.

func Variable() int {
	n := 1
	_ = 1
	_ = 2
	_ = 3
	_ = 4
	_ = 5
	_ = 6
	return n
}

func Constant() int {
	const c = 1
	_ = 1
	_ = 2
	_ = 3
	_ = 4
	_ = 5
	_ = 6
	return c
}

func Parameter(p int) int {
	_ = 1
	_ = 2
	_ = 3
	_ = 4
	_ = 5
	_ = 6
	return p
}

func ReturnValue() (r int) {
	_ = 1
	_ = 2
	_ = 3
	_ = 4
	_ = 5
	_ = 6
	r = 1
	return
}

func TypeParameter[T any](v T) T {
	_ = 1
	_ = 2
	_ = 3
	_ = 4
	_ = 5
	_ = 6
	var out T
	out = v
	return out
}
