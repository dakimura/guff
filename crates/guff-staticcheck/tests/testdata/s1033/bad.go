package main

func f(m map[string]int, k string) {
	if _, ok := m[k]; ok {
		delete(m, k)
	}
}
