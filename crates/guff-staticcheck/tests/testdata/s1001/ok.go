package main

func f(dst, src []int) {
	copy(dst, src)
}

// A repeated binding in upstream's pattern is compared, not re-bound: the
// `src` in `(IndexExpr src key)` has to be the same tree the loop header bound.
// Each of these three ranges (or counts) over the *destination*, or indexes a
// different container on the right, so none of them matches — thanos'
// `pkg/compact/planner_test.go` is the first shape, three times over.

type holder struct{ metas []int }

var c holder
var d holder

func rangeOverDst(dst, src []int) {
	for i := range dst {
		dst[i] = src[i]
	}
}

func lenOfDst(dst, src []int) {
	for i := 0; i < len(dst); i++ {
		dst[i] = src[i]
	}
}

func differentSelector(dst []int) {
	for i := range c.metas {
		dst[i] = d.metas[i]
	}
}
