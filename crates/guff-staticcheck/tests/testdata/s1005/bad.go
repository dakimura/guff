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

// Not findings since golangci-lint 2.14.0 (staticcheck v0.8.1). v0.7.0's first
// pattern was
//
//	(AssignStmt [_ (Ident "_")] _ (Or (IndexExpr _ _) (UnaryExpr "<-" _)))
//
// and reported both of these; v0.8.1 dropped the IndexExpr arm, arguing that
// `x, _ = m[k]` may say "there might be no entry, and I don't care". They stay
// in this file so the golden case keeps recording that upstream is silent here.
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
