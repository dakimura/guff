package main

func f(dst, src []int) {
	for i, v := range src {
		dst[i] = v
	}
}

// Until 2026-08-27 this fixture held only the slice-to-slice range above, so
// upstream's other branches — the array shapes that decide between `copy()` and
// a plain assignment, and the `[:]` its message and its fix add — were measured
// by nothing.

func indexForm(dst, src []int) {
	for i := range src {
		dst[i] = src[i]
	}
}

func threeClause(dst, src []int) {
	for i := 0; i < len(src); i++ {
		dst[i] = src[i]
	}
}

func arrayToArray(dst, src [4]int) {
	for i, v := range src {
		dst[i] = v
	}
}

func arrayToSlice(dst []int, src [4]int) {
	for i, v := range src {
		dst[i] = v
	}
}

func sliceToArray(dst [4]int, src []int) {
	for i, v := range src {
		dst[i] = v
	}
}

func pointerToArray(dst, src *[4]int) {
	for i, v := range src {
		dst[i] = v
	}
}
