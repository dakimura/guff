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

// The other half of upstream's first pattern:
//
//	(AssignStmt [_ (Ident "_")] _ (Or (IndexExpr _ _) (UnaryExpr "<-" _)))
//
// guff carried only the receive arm, so a map index with a discarded comma-ok
// went unreported in either assignment token. boundary's
// internal/plugin/loopback writes two of them.
//
// Read the *pinned* v0.7.0 for this: the tip has since dropped the IndexExpr
// arm, with a comment arguing that `x, _ = m[k]` may be deliberate.
func mapIndexDeclare(m map[string]int) int {
	v, _ := m["k"]
	return v
}

func mapIndexAssign(m map[string]int) int {
	var v int
	v, _ = m["k"]
	return v
}

// The receive arm in its two-name form; the file only had `_ = <-ch`.
func receiveDeclare(ch chan int) int {
	v, _ := <-ch
	return v
}
