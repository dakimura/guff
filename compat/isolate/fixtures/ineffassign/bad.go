package p

func Bad() int {
	x := 1
	x = 2 // ineffectual assignment
	_ = x
	return x
}

// A composite-literal key that spells a local. go/parser cannot tell a struct
// field name from a map key, so upstream ineffassign resolves the key in scope
// and counts it as a *use* of `lookback` — the store below is not reported.
func FieldKeyShadowsLocal(n int) *cfg {
	lookback := n
	if lookback < 0 {
		lookback = 1
	}
	return &cfg{lookback: n}
}

// The same shape with a key naming a different field: the store really is dead
// and both tools report it.
func FieldKeyDoesNot(n int) *cfg {
	lookback := n
	if lookback < 0 {
		lookback = 1
	}
	return &cfg{other: n}
}

type cfg struct {
	lookback int
	other    int
}
