package main

func f(m map[string]int, b []byte) {
	k := string(b)
	_ = m[k]
	_ = m[k]
}

// One lookup is enough: upstream has no count threshold, only the requirement
// that every referrer of the conversion is a map read.
func one(m map[string]int, b []byte) int {
	k := string(b)
	return m[k]
}
