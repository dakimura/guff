package main

func f(m map[string][]int, k string, v int) {
	if _, ok := m[k]; ok {
		m[k] = append(m[k], v)
	} else {
		m[k] = []int{v}
	}
}

// The `+=` and `++` alternatives of upstream's query. Both were unreachable in
// guff until 2026-08-13: the guard compared the two index expressions by AST
// *node id*, which two distinct nodes never share, so the arms could not fire
// and no fixture noticed (COMPAT-HARDENING §4).
func counters(m map[string]int, k string, v int) {
	if _, ok := m[k]; ok {
		m[k] += v
	} else {
		m[k] = v
	}

	if _, ok := m[k]; ok {
		m[k]++
	} else {
		m[k] = 1
	}
}
