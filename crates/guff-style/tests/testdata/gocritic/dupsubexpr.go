package gocritic

// dupSubExpr:
//
//	if !c.opSet[expr.Op] { return }
//	if c.resultIsFloat(expr.X) && c.floatOpsSet[expr.Op] { return }
//	if typep.SideEffectFree(c.ctx.TypesInfo, expr) && c.opSet[expr.Op] &&
//		astequal.Expr(expr.X, expr.Y) { warn }
//
// guff kept the operator set and the AST comparison and had *neither* guard,
// so it reported `f == f` (the NaN test), `f - f` (signed-zero normalisation)
// and `rand.Int() == rand.Int()`. Its expression equality also carried an
// operator whitelist of its own that upstream has no trace of, which hid
// `+a == +a` — and hid `(<-ch) == (<-ch)`, the false positive the missing
// `SideEffectFree` would otherwise have produced.

type dupSubFloat float64

type dupSubAlias = float64

func dupSubImpure() int { return 0 }

// Every expression here is a finding.
func dupSubExprBad(
	i int,
	f float64,
	df dupSubFloat,
	af dupSubAlias,
	p *int,
	m map[string]int,
	s []int,
) {
	_ = i == i
	_ = +i == +i
	_ = ^i == ^i
	_ = *p == *p
	_ = m["k"] == m["k"]
	_ = s[1:2][0] == s[1:2][0]
	_ = []int{i}[0] == []int{i}[0]
	// `resultIsFloat` is a `*types.Basic` type assertion, so it sees through
	// neither a defined type nor an alias: both of these are findings.
	_ = df/df == 1
	_ = af == af
	// `<` is not one of the six float-sensitive operators.
	_ = f < f
	// A conversion is the one call `SideEffectFree` accepts.
	_ = int64(i) == int64(i)
}

// Every expression here is silent.
func dupSubExprOK(f float64, ch chan int, i int) {
	// The six operators where two equal float operands can still be meaningful.
	_ = f == f
	_ = f != f
	_ = f <= f
	_ = f >= f
	_ = f/f == 1
	_ = f-f == 0
	// Not side-effect free: two calls, or two receives, are not one value.
	_ = dupSubImpure() == dupSubImpure()
	_ = (<-ch) == (<-ch)
	// Not one of the watched operators.
	_ = i+i == 0
	_ = i*i == 0
}
