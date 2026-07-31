package main

func ok() {
	var v int
	y := &v
	_ = *y
}

// Nil-check on a map behind a pointer must not flag the pointer deref used to
// inspect/assign the map (prometheus Annotations pattern).
type M map[string]int

func okMapPtr(a *M) {
	if *a == nil {
		*a = M{}
	}
	(*a)["k"] = 1
}
