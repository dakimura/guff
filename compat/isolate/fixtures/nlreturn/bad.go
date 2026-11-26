package p

// nlreturn names the keyword it wants a blank line before, so `return`,
// `break`, `continue` and `goto` are four different sentences.

func Return(cond bool) int {
	if cond {
		return 1
	}
	x := 2
	return x
}

func Break(xs []int) {
	for _, x := range xs {
		if x > 0 {
			_ = x
			break
		}
	}
}

func Continue(xs []int) {
	for _, x := range xs {
		if x > 0 {
			_ = x
			continue
		}
	}
}

func Goto() {
	i := 0
	_ = i
	goto end
end:
}
