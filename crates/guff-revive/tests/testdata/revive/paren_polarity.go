// Package parenpolarity pins the direction revive reads parentheses in.
//
// honnef matches through `pattern`, which strips `*ast.ParenExpr` at every
// level and before it binds; revive uses plain type assertions and
// `astutils.GoFmt` (= `go/printer`), neither of which unwraps anything. guff
// had the staticcheck polarity in three revive rules, so it reported findings
// upstream does not and rendered messages upstream does not write.
//
// Found by `compat/fuzz.py --allow-dirty-seeds --case revive`
// (docs/COMPAT-HARDENING.md §4, 2026-08-13).
package parenpolarity

import "fmt"

// unnecessary-format: reported bare, silent once the format string is
// parenthesized (`astutils.IsStringLiteral` is a bare `.(*ast.BasicLit)`).
func bareFormat() error { return fmt.Errorf("clean error") }

func parenthesizedFormat() error { return fmt.Errorf(("clean error")) }

// The table is keyed on the *printed* callee, so a parenthesis in it renders
// to something that is not a key.
func parenthesizedCallee() error { return (fmt.Errorf)("clean error") }

// use-fmt-print renders the arguments with GoFmt, which keeps parentheses.
func printlnParenthesized() { println(("ok")) }

// redefines-builtin-id reports the GenDecl, i.e. the `var` keyword — not the
// name. The two only differ once the declaration is not a short one.
func shadowShort() {
	len := 1
	_ = len
}

func shadowVar() {
	var len int = 1
	_ = len
}
