package v1

func first2(xs []string) map[string]int {
	out := map[string]int{}
	for i, x := range xs {
		if len(x) > 3 {
			out[x] = i * 2
		} else if x == "" {
			out["empty"]++
		} else {
			out[x] = len(x) + i
		}
	}
	return out
}

func second2(m map[string]int) []string {
	var keys []string
	for k, v := range m {
		if v > 5 {
			keys = append(keys, k+"!")
		} else if v < 0 {
			keys = append(keys, "-"+k)
		} else {
			keys = append(keys, k)
		}
	}
	return keys
}