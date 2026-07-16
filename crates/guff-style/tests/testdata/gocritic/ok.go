package gocritic

func okElseIf(cond1, cond2 bool) {
	if cond1 {
		println("a")
	} else if cond2 {
		println("b")
	}
}

func okSwitch(x int) {
	switch x {
	case 1:
		println(1)
	case 2:
		println(2)
	default:
		println("d")
	}
}

func okSwitchTagless(cond, other bool) {
	switch {
	case cond:
		println("ok")
	case other:
		println("other")
	}
}

func okLen(s []int) {
	_ = len(s) == 0
	_ = len(s) > 0
}

func okSlice(s []int) {
	_ = s
	_ = s[1:]
}

func okNew() {
	_ = new(bool)
}

func okAppend(xs []int) {
	xs = append(xs, 1)
	_ = xs
}

func okCapt(in int) (out int) {
	return in
}

func okAssign(x int) {
	x++
	x *= 2
	_ = x
}

func okUnderef(p *struct{ N int }) {
	_ = p.N
}
