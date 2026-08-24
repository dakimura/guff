// Package typerender pins how revive renders a type into a message.
//
// Upstream renders with `gofmt` (`rule/utils.go`) and `file.Render`
// (`lint/file.go`) — both are `printer.Fprint`, i.e. go/printer. guff used to
// approximate that with a five-arm walker that answered the literal string
// "<type>" for map, chan, func, variadic and generic types and collapsed a
// non-empty `interface{ ... }` to `interface{}`. Those are the shapes here.
//
// This is a file of its own rather than lines appended to extended_bad.go:
// that file's test asserts on the whole message list, and new exported
// functions there displace unrelated assertions.
package typerender

import "time"

// `enforce-repeated-arg-type-style: short` quotes the repeated type, so each
// pair below yields that type's rendered spelling.

// RepeatedMap has a repeated map type.
func RepeatedMap(a map[string]int, b map[string]int) {}

// RepeatedChan has a repeated channel type.
func RepeatedChan(a chan int, b chan int) {}

// RepeatedFunc has a repeated func type.
func RepeatedFunc(a func(int) error, b func(int) error) {}

// RepeatedIface has a repeated non-empty interface type.
func RepeatedIface(a interface{ Foo() int }, b interface{ Foo() int }) {}

// RepeatedPtrSlice has a repeated slice-of-pointer type.
func RepeatedPtrSlice(a []*time.Time, b []*time.Time) {}

// Pair is generic, so its instantiation is an IndexListExpr.
type Pair[K comparable, V any] struct {
	K K
	V V
}

// RepeatedGeneric has a repeated instantiated generic type.
func RepeatedGeneric(a Pair[string, int], b Pair[string, int]) {}

// time-equal is deliberately absent. Upstream gates the whole rule behind
// `file.Pkg.TypeCheck() != nil`, and under golangci-lint that type-check uses
// `importer.Default()` — the gc export-data importer, which finds no .a files
// on a modern toolchain. Every import resolves to invalid, so `time.Time` is
// never recognised and the rule is silent upstream no matter what is written
// here. See this case's ratchet.json and COMPAT-HARDENING §6; the rendering of
// its message is pinned by a Rust test instead.

// VarDeclNonTrivial has var declarations whose type var-declaration wants
// dropped, and quotes while doing so.
func VarDeclNonTrivial() {
	var m map[string]int = map[string]int{}
	var c chan struct{} = make(chan struct{})
	_, _ = m, c
}
