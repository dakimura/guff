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
