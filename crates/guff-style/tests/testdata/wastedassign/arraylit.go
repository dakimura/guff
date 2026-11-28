package arraylit

// Array literals assigned to a variable. go/ssa (x/tools v0.52), which
// wastedassign builds, copies every literal through a temporary and stores it
// to the variable once, so each overwritten value is reported at the
// variable. staticcheck's IR fills a partial literal in place instead; guff
// does that only for the IR staticcheck reads, never for this build.

func g() int { return 1 }

func definePartial() [3]int {
	x := [3]int{g()}
	x = [3]int{1, 2, 3}
	return x
}

func existingPartial() [3]int {
	var ar [3]int
	_ = ar
	ar = [3]int{g()}
	ar = [3]int{1, 2, 3}
	return ar
}

func redeclaredPartial() int {
	ar := [3]int{1, 2, 3}
	n, ar := 0, [3]int{g()}
	return ar[1] + n
}

func full() [3]int {
	x := [3]int{g(), 2, 3}
	x = [3]int{1, 2, 3}
	return x
}
