package main

func f(dst, src []int) {
	for i, v := range src {
		dst[i] = v
	}
}
