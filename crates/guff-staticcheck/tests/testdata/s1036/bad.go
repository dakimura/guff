package main

func f(m map[string][]int, k string, v int) {
	if _, ok := m[k]; ok {
		m[k] = append(m[k], v)
	} else {
		m[k] = []int{v}
	}
}
