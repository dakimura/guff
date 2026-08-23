package ok

// The same shape with a key that does *not* name the local: nothing binds
// `lookback` after the assignment, so it stays ineffectual on both sides.
func unshadowedFieldKey(n int) *C {
	lookback := n
	if lookback < 0 {
		lookback = 1
	}
	return &C{other: n}
}

type C struct {
	lookback int
	other    int
}
