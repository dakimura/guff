package main

func f(ch chan int) {
	_ = <-ch
}

// The three range shapes. Until 2026-08-27 this fixture held only the channel
// receive above, so guff's handling of these — and, once it had one, its
// suggested fix for them — was measured by nothing.
//
// A range whose left side is only blanks cannot use `:=`: the compiler rejects
// `for _ := range` and `for _, _ := range` with "no new variables on left side
// of :=". That is why upstream's `rs.TokPos + 1` deletion is safe.
func rangeBlankKey(xs []int) {
	for _ = range xs {
		_ = 1
	}
}

func rangeBlankBoth(xs []int) {
	for _, _ = range xs {
		_ = 1
	}
}

func rangeBlankValue(xs []int) {
	for i, _ := range xs {
		_ = i
	}
}
