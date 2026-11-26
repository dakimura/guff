package p

// `inline` reports at a call to a `//go:fix inline` function whose type
// arguments the call site does not spell out:
//
//	typeArgs := st.typeArguments(caller.Call)
//	if len(typeArgs) != len(callee.TypeParams) {
//		return nil, fmt.Errorf("cannot inline: type parameter inference is not yet supported")
//	}
//
// guff had a hard-coded table of the x/exp names, so vitess's own
// `ptr.Of` — sixteen calls across five packages — drew nothing. The directive
// is read from the imported package's source, which is right there.
//
// Measured against golangci-lint 2.12.2: four of the eight below report.

import "example.com/govet/inlinegofixlocal/ptr"

type S struct{ P *int }

// Reported: inference.
func A() *int { return ptr.Of(1) }

// Silent: the type argument is spelled out. (Upstream then inlines, and
// suppresses the result because it literalizes.)
func B() *int { return ptr.Of[int](1) }

// Silent: both type arguments spelled.
func C() (int, string) { return ptr.Pair[int, string](1, "a") }

// Reported: neither spelled.
func D() (int, string) { return ptr.Pair(1, "a") }

// Reported: only one of two spelled.
func E() (int, string) { return ptr.Pair[int](1, "a") }

// Silent here: not generic, so the message does not apply. Upstream says
// "Call of ptr.Plain should be inlined" instead — the inliner half guff does
// not have.
func F() int { return ptr.Plain(1) }

// Silent: generic, but no directive on the declaration.
func G() *int { return ptr.NotMarked(1) }

// Reported: a call inside a composite literal is still a call.
func H() S { return S{P: ptr.Of(2)} }
