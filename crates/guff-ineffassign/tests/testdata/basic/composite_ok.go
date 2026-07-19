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
