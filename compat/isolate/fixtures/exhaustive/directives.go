package p

// `//exhaustive:` comment directives, and the `explicit-exhaustive-switch` /
// `explicit-exhaustive-map` settings that invert them.
//
// Every shape below was measured against golangci-lint 2.12.2 under three
// configs, all three of which read this file:
//
//	compat/golden/cases/exhaustive                        (plain)
//	compat/golden/cases/exhaustive-explicit               (explicit-* true)
//	compat/golden/cases/exhaustive-default-case-required  (default-case-required true)
//
// The three disagree about most of these switches and literals, which is the
// point: a directive that is honoured in one config has to be *ignored* in
// another, and a single config cannot tell the two apart.

// ---- switch directives ----------------------------------------------------

// Baseline: no directive at all.
func dirSwitchPlain(c Color) string {
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

func dirSwitchIgnore(c Color) string {
	//exhaustive:ignore
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

func dirSwitchEnforce(c Color) string {
	//exhaustive:enforce
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

// `userDirectives` maps each comment to the *longest* directive it starts
// with, so this is not also an `//exhaustive:enforce`: under
// `explicit-exhaustive-switch` the switch goes unchecked, and without it the
// switch is checked with the default case demanded.
func dirSwitchEnforceDefaultCase(c Color) string {
	//exhaustive:enforce-default-case-required
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

// The mirror image: all three members are listed, so the only thing left to
// report is the missing default case — which this directive waives.
func dirSwitchIgnoreDefaultCase(c Color) string {
	//exhaustive:ignore-default-case-required
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	case Blue:
		return "b"
	}
	return ""
}

// Same switch without the directive, so `default-case-required` has something
// to report and the case above is not just a fixture that never fires.
func dirSwitchAllMembers(c Color) string {
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	case Blue:
		return "b"
	}
	return ""
}

// Conflicting directives: upstream writes the second `if` without an `else`,
// so enforcing wins over ignoring for the default case. The plain
// `//exhaustive:ignore` still skips the switch entirely unless
// `explicit-exhaustive-switch` is set, and under that setting the
// `//exhaustive:enforce` selects it.
func dirSwitchIgnoreAndEnforce(c Color) string {
	//exhaustive:ignore
	//exhaustive:enforce
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

// The directive test is a prefix test, so a longer word still matches.
func dirSwitchIgnorePrefix(c Color) string {
	//exhaustive:ignoreme
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

func dirSwitchEnforcePrefix(c Color) string {
	//exhaustive:enforcement
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

// A trailing comment on the `switch` line is not associated with the switch
// statement by `ast.NewCommentMap`, so it enforces nothing.
func dirSwitchTrailing(c Color) string {
	switch c { //exhaustive:enforce
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

// ---- map literal directives -----------------------------------------------

var dirMapPlain = map[Color]string{
	Red:   "r",
	Green: "g",
}

//exhaustive:ignore
var dirMapIgnore = map[Color]string{
	Red:   "r",
	Green: "g",
}

//exhaustive:enforce
var dirMapEnforce = map[Color]string{
	Red:   "r",
	Green: "g",
}

// The map checker asks `hasCommentPrefix` rather than `userDirectives`, with
// no longest-first disambiguation — so for a map literal, and unlike a switch,
// the default-case directives are read as plain ignore/enforce.
//
//exhaustive:ignore-default-case-required
var dirMapIgnoreDefaultCase = map[Color]string{
	Red:   "r",
	Green: "g",
}

//exhaustive:enforce-default-case-required
var dirMapEnforceDefaultCase = map[Color]string{
	Red:   "r",
	Green: "g",
}

//exhaustive:ignoreme
var dirMapIgnorePrefix = map[Color]string{
	Red:   "r",
	Green: "g",
}

func dirMapReturn() map[Color]string {
	//exhaustive:enforce
	return map[Color]string{
		Red:   "r",
		Green: "g",
	}
}

func dirMapAssign() int {
	//exhaustive:enforce
	m := map[Color]string{
		Red:   "r",
		Green: "g",
	}
	return len(m)
}

func dirMapCallArg() int {
	//exhaustive:enforce
	return mapLen(map[Color]string{
		Red:   "r",
		Green: "g",
	})
}

func mapLen(m map[Color]string) int { return len(m) }

// A directive on the enclosing `FuncDecl` reaches nothing: `FuncDecl` is not
// one of the node kinds whose comments the map checker folds in.
//
//exhaustive:enforce
func dirMapFuncDoc() map[Color]string {
	return map[Color]string{
		Red:   "r",
		Green: "g",
	}
}

// Upstream's stack walk reads `default: break`, and its comment claims it
// stops at the first node that is not in its list — but `break` inside a
// `switch` leaves the `switch`, not the `for`. The walk therefore runs to the
// top of the stack, and this directive on the `var` reaches a literal nested
// inside an immediately-invoked func literal, across the `FuncLit`,
// `BlockStmt` and `ReturnStmt` in between.
//
//exhaustive:enforce
var dirMapThroughFuncLit = func() map[Color]string {
	return map[Color]string{
		Red:   "r",
		Green: "g",
	}
}()

//exhaustive:ignore
var dirMapIgnoreThroughFuncLit = func() map[Color]string {
	return map[Color]string{
		Red:   "r",
		Green: "g",
	}
}()

// The outer literal is ignored; the inner one inherits the same comment
// through the stack, so it is skipped too.
//
//exhaustive:ignore
var dirMapNested = map[Color]map[Color]string{
	Red: {
		Red:   "r",
		Green: "g",
	},
	Green: {},
}
