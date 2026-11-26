package gosec_g602

// G602 is the one SSA analyzer among the gosec rules guff implements
// (securego/gosec analyzers/slice_bounds.go), and the only one bad.go does not
// reach. Kept in its own file so the golden case can give it its own package,
// like every other fixture here.

func sliceBoundsOutOfRange() byte {
	s := make([]byte, 0)
	t := s[:3]
	return t[0]
}

func sliceIndexOutOfRange() byte {
	s := make([]byte, 2)
	return s[5]
}

func sliceBoundsAfterReslice() byte {
	s := make([]byte, 10)
	s = s[:2]
	return s[4]
}

func guardedByLen(n int) byte {
	s := make([]byte, n)
	if len(s) > 3 {
		return s[3]
	}
	return 0
}

// --- where the capacity comes from -----------------------------------------
//
// Upstream learns a slice's size from an `Alloc` of a fixed-size array, which
// is what go/ssa builds for `make([]T, constN)`. guff emits a `MakeSlice`
// instead and bridges the gap by reading a constant off it — but it read the
// **length**, and `make([]T, 0, cap)` has capacity `cap`, not 0. Every index
// into such a slice was a finding; photoprism's
// `pkg/vector/alg/json_importer.go` is where that showed.

func makeLenAndCapOutOfRange() byte {
	s := make([]byte, 2, 4) // FINDING at s[5]: 5 is past the capacity
	return s[5]
}

func makeZeroLenConstCapInRange() float64 {
	g := make([]float64, 0, 8) // silent: 0 is inside the capacity
	g[0] = 1
	return g[0]
}

func makeZeroLenVarCap(n int) float64 {
	g := make([]float64, 0, n) // silent: a non-constant capacity is not tracked
	g[0] = 1
	return g[0]
}
