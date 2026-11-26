package p

// makezero has two messages: appending to a slice that was made with a non-zero
// length, and (under `always`) declaring one at all.

func AppendToNonZeroLen() []int {
	x := make([]int, 10)
	x = append(x, 1)
	return x
}

func DeclaredWithLength() []int {
	// `always: true` is what makes this one reportable on its own.
	y := make([]int, 5)
	return y
}
