// Package emptyblockrange pins where `empty-block` stops walking.
//
// Upstream (revive v1.15.0, `rule/empty_block.go`) prunes the subtree of a
// range statement **only when its body is empty**, where the walk would
// otherwise reach the same `BlockStmt` and report it a second time:
//
//	case *ast.RangeStmt:
//		if len(n.Body.List) == 0 {
//			w.onFailure(…)
//			return nil // skip visiting the range subtree
//		}
//
// guff pruned at *every* range statement, so nothing inside a non-empty
// `for … range` was ever visited. k6's `ramping_arrival_rate_test.go:294` is an
// empty drain loop two closures deep inside `for _, tc := range tests`, and the
// `//nolint:revive` over it was reported as an unused directive.
package emptyblockrange

func call() bool { return false }

// The range arm itself. An empty body is one finding, at the `for`.
func emptyRange(ch chan int) {
	for range ch {
	}
}

func emptyRangeKeyOnly(s []int) {
	for range s {
	}
}

// A non-empty range body must still be walked. Every block below is a finding.
//
// The shapes are spread over several small functions on purpose: the golden
// case also runs `cognitive-complexity`, and one function holding all of them
// scores high enough to fire it — which would put an unrelated rule's count in
// this fixture's expectations.
func insideRangeIf(s []int, n int) {
	for _, v := range s {
		_ = v
		if n > 0 {
		}
	}
}

func insideRangeRange(s []int, ch chan int) {
	for _, v := range s {
		_ = v
		for range ch {
		}
	}
}

func insideRangeForever(s []int) {
	for _, v := range s {
		_ = v
		for {
		}
	}
}

func insideRangeForCond(s []int, n int) {
	for _, v := range s {
		_ = v
		for n > 0 {
		}
	}
}

func insideRangeForClause(s []int) {
	for _, v := range s {
		_ = v
		for i := 0; i < 1; i++ {
		}
	}
}

func insideRangeBareBlock(s []int) {
	for _, v := range s {
		_ = v
		{
		}
	}
}

func insideRangeFuncLit(s []int, ch chan int) {
	for _, v := range s {
		_ = v
		go func() {
			for range ch {
			}
		}()
	}
}

// Two range levels deep.
func nestedRanges(s []int, ch chan int) {
	for _, v := range s {
		_ = v
		for _, w := range s {
			_ = w
			for range ch {
			}
		}
	}
}

// The ignore arms still apply inside a range: a function literal's body, a
// `select` body, and a `for cond()` whose condition is a call.
func ignoresInsideRange(s []int) {
	for _, v := range s {
		_ = v
		f := func() {}
		_ = f
		for call() {
		}
		select {}
	}
}

// A range with a key and a non-empty body is silent, and so is an empty
// function body.
func silent(s []int) {
	for i := range s {
		_ = i
	}
}

func emptyFunc() {}
