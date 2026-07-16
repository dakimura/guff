package revivetest

// With preserveScope: should NOT flag (initializer would enlarge scope mid-block).
func keepScopeMidBlock(flag bool) int {
	x := 1
	if ok := flag; ok {
		return x
	} else {
		y := 2
		return y
	}
	return x
}

// Without / with preserveScope at block end: still OK to flag (scope ends either way).
func dropElseAtEnd(flag bool) int {
	if flag {
		return 1
	} else {
		return 2
	}
}
