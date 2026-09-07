package main

// Both forms upstream matches, and the four near-misses it does not.
//
// The nested form is `checkAssertNotNilFn2Q`, and its pattern pins more than the
// shape of the two conditions:
//
//	(IfStmt nil (BinaryExpr lhs "!=" nil)
//	    [ifstmt@(IfStmt (AssignStmt [_ ok] _ [(TypeAssertExpr lhs _)]) ok _ nil)]
//	    nil)
//
// The outer `if` has no init and no else, its body is a one-element list, and
// the inner `if` has no else either. Rewriting the code the message suggests
// would delete an else branch, so the nil check is not redundant when one is
// there — which is why upstream stays quiet on shapes 4 and 5.
//
// Upstream also reports the nested form on the *inner* `if` (the pattern's own
// `ifstmt` binding), not on the outer one the walk matched from.

type T struct{}

func use(...any) {}

// 1. flat: `ok && i != nil`
func flat(i any) {
	if _, ok := i.(*T); ok && i != nil {
		use(ok)
	}
}

// 2. flat, operands the other way round
func flatSwapped(i any) {
	if _, ok := i.(*T); i != nil && ok {
		use(ok)
	}
}

// 3. nested, both ifs bare
func nested(i any) {
	if i != nil {
		if _, ok := i.(*T); ok {
			use(ok)
		}
	}
}

// 4. nested, but the inner if has an else — hashicorp/packer
// command/build.go:304 is this shape
func nestedInnerElse(i any) {
	if i != nil {
		if _, ok := i.(*T); ok {
			use(ok)
		} else {
			use(i)
		}
	}
}

// 5. nested, but the outer if has an else
func nestedOuterElse(i any) {
	if i != nil {
		if _, ok := i.(*T); ok {
			use(ok)
		}
	} else {
		use(i)
	}
}

// 6. nested, but the outer body holds a second statement
func nestedTwoStmts(i any) {
	if i != nil {
		if _, ok := i.(*T); ok {
			use(ok)
		}
		use(i)
	}
}

// 7. nested, but the outer if has an init
func nestedOuterInit(m map[string]any) {
	if i := m["k"]; i != nil {
		if _, ok := i.(*T); ok {
			use(ok)
		}
	}
}

// 8. flat, with the ok variable named something else
func flatRenamed(i any) {
	if _, yes := i.(*T); yes && i != nil {
		use(yes)
	}
}
