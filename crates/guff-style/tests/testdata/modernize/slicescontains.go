package slicescontains

func containsReturn(s []int, needle int) bool {
	for _, v := range s {
		if v == needle {
			return true
		}
	}
	return false
}

func containsIndexElem(s []int, needle int) bool {
	for i := range s {
		if s[i] == needle {
			return true
		}
	}
	return false
}

func containsFuncPred(s []int) bool {
	pred := func(v int) bool { return v > 0 }
	for _, v := range s {
		if pred(v) {
			return true
		}
	}
	return false
}

func containsBreakBody(s []int, needle int) {
	for _, v := range s {
		if v == needle {
			println("found")
			break
		}
	}
}

func containsFoundAssign(s []int, needle int) bool {
	found := false
	for _, v := range s {
		if v == needle {
			found = true
			break
		}
	}
	return found
}

func skipSoleBreak(s []int, needle int) {
	for _, v := range s {
		if v == needle {
			break
		}
	}
}

func alreadyContains(s []int, needle int) bool {
	return contains(s, needle)
}

func contains(s []int, needle int) bool {
	for _, v := range s {
		_ = v
		_ = needle
	}
	return false
}
