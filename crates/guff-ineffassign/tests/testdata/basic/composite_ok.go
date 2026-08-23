package ok

type T struct{ P *int }

// escapeInComposite mirrors prometheus config.go: a local used by address
// inside a composite literal. `x` is live (via `&x`), so its assignment is not
// ineffectual — regression for missing composite-literal traversal.
func escapeInComposite() *T {
	x := 1
	return &T{P: &x}
}

// useAsElement uses a local as a plain composite-literal element value.
func useAsElement() []int {
	y := 2
	return []int{y}
}

// useAsMapKey uses a local as a map-literal key.
func useAsMapKey() map[int]string {
	k := 3
	return map[int]string{k: "v"}
}

// useInTypeAssert mirrors config.go checkStaticTargets: a range variable used
// only in a type assertion `cfg.(T)`. `v` is live — not ineffectual.
func useInTypeAssert(vals []any) {
	for _, v := range vals {
		if s, ok := v.(string); ok {
			print(s)
		}
	}
}

// useInSliceIndex mirrors openmetricsparse.go: locals used only as slice
// bounds `s[a:b]`. `a` and `b` are live — not ineffectual.
func useInSliceIndex(s string, off []int) string {
	a := off[0]
	b := off[1]
	return s[a:b]
}

// steppedLoop: the post-increment `i += 2` feeds the next iteration's condition
// and body, so it is not ineffectual. Regression for a for-loop CFG back-edge
// that linked cond->cond (a no-op self loop) instead of post->cond, leaving the
// increment with no successor use.
func steppedLoop(xs []int) int {
	sum := 0
	for i := 0; i < len(xs); i += 2 {
		sum += xs[i]
	}
	return sum
}

// shadowedFieldKey mirrors grafana's kvStorageBackend: a local named exactly
// like a struct field, dead after its last assignment, in a function whose
// return literal keys that field. go/parser cannot tell a field name from a
// map key, so it binds the key ident to the local in scope — and upstream
// ineffassign reads `id.Obj`, so it counts the key as a use and stays quiet.
// go/types knows better; following it here reports what upstream does not.
func shadowedFieldKey(opts struct{ Lookback int }) *B {
	lookback := opts.Lookback
	if lookback < 0 {
		lookback = 1
	}
	return &B{lookback: opts.Lookback}
}

type B struct{ lookback int }
