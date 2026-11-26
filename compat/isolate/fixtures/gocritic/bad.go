package p

func Bad(s string, xs []int) {
	_ = len(s) == 0 // emptyStringTest: prefer s == ""
	x := 1
	x = x + 1 // assignOp: prefer x++
	_ = x
	_ = len(xs) >= 0 // sloppyLen
	_ = *new(int)    // newDeref
	switch true { // switchTrue + singleCaseSwitch
	case true:
	}
}
