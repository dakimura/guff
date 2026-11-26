package p

func MinRewrite(a, b int) int {
	var x int
	if a < b {
		x = a
	} else {
		x = b
	}
	return x
}

func RangeInt(n int) {
	for i := 0; i < n; i++ {
		_ = i
	}
}

func MapsClone(m map[string]int) map[string]int {
	out := make(map[string]int, len(m))
	for k, v := range m {
		out[k] = v
	}
	return out
}
