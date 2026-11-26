// Package cognitivecomplexity pins revive's cognitive-complexity arithmetic.
//
// The counter is `rule/cognitive_complexity.go`. Its `walk` adds
// `increment + nestingLevel` and then raises the nesting for the children it
// is given — and it is given *specific* children, not the whole node:
//
//	case *ast.ForStmt:  targets := []ast.Node{n.Cond, n.Body}  // not Init/Post
//	case *ast.RangeStmt: v.walk(1, n.Body)                     // not Key/Value/X
//	case *ast.SwitchStmt: v.walk(1, n.Body)                    // not Init/Tag
//	case *ast.FuncLit:   v.walk(0, n.Body)                     // 0 + nesting
//
// and `walkIfElse` walks `Cond` and `Body` — never `Init` — charging 1 for each
// `else if` and **nothing** for a plain trailing `else`.
//
// Every function below carries its measured complexity in a comment.
package cognitivecomplexity

func take(f func()) { f() }

// 1 — the ranged expression is not walked, so its `&&` costs nothing.
func rangeSubject(a, b bool, m map[bool][]int) {
	for range m[a && b] {
	}
}

// 1 — neither a `for`'s Init nor its Post is walked.
func forInitPost(a, b bool) {
	for x := a && b; x; x = a && b {
	}
}

// 1 — nor a `switch`'s tag.
func switchTag(a, b bool) {
	switch a && b {
	case true:
	}
}

// 1 — nor an `if`'s Init.
func ifInit(a, b bool) {
	if x := a && b; x {
	}
}

// 2 — but the `if`'s own condition is.
func ifCond(a, b bool) {
	if a && b {
	}
}

// 2 — a function literal adds `0 + nesting`, so one loop deep it costs 1.
func funcLitOneDeep(s []int) {
	for range s {
		f := func() {}
		_ = f
	}
}

// 5 — two loops deep: 1 + (1+1) + (0+2).
func funcLitTwoDeep(s []int) {
	for range s {
		for range s {
			f := func() {}
			_ = f
		}
	}
}

// 3 — an if / else-if / else-if / else chain: 1 for the `if`, 1 for each
// `else if`, and nothing for the trailing `else`.
func elseIfChain(n int) {
	if n == 1 {
	} else if n == 2 {
	} else if n == 3 {
	} else {
	}
}

// 3 — an `else` holding a nested `if` is not an `else if`: the inner one pays
// the nesting instead.
func elseWithIf(n int) {
	if n == 1 {
	} else {
		if n == 2 {
		}
	}
}

// 4 — a labelled `break` costs 1 on top of the two loops.
func labeledBreak(s []int) {
outer:
	for range s {
		for range s {
			break outer
		}
	}
}

// 6 — three nested ifs: 1 + 2 + 3.
func nestedIf(n int) {
	if n > 0 {
		if n > 1 {
			if n > 2 {
			}
		}
	}
}

// 21 — the mix: loops, ifs, a function literal and a binary expression, all
// paying their nesting.
func deepMix(s []int, a, b bool) {
	for range s {
		if a {
			for range s {
				if b {
					take(func() {
						if a && b {
						}
					})
				}
			}
		}
	}
}

// 8 — over the default limit of 7 without any of the arms above.
func overDefault(n int) {
	if n == 1 {
	}
	if n == 2 {
	}
	if n == 3 {
	}
	if n == 4 {
	}
	if n == 5 {
	}
	if n == 6 {
	}
	if n == 7 {
	}
	if n == 8 {
	}
}
