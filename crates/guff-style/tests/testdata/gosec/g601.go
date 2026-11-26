// Package g601 is the G601 fixture: implicit memory aliasing in a `range`
// loop, which gosec reports only below Go 1.22.
//
// Every function is one shape, and the trailing comment says what
// golangci-lint 2.12.2 (gosec v2.26.1) does with it in a `go 1.18` module:
// FIRES or silent. Most of the answers follow from upstream being a stateful
// preorder walk rather than a question about a loop — see gosec_g601.rs.
package g601

type T struct {
	f int
	g struct{ h int }
}

// P is a named pointer type and A an alias of one: to go/types neither is a
// *types.Pointer, so a selector through either still fires.
type P *T
type A = *T

func sink(...interface{}) {}
func take(*int) *int      { return nil }

func s01(xs []int) { for _, v := range xs { sink(&v) } }  // FIRES: plain &v
func s02(xs []T) { for _, v := range xs { sink(&v.f) } }  // FIRES: &v.f, v struct
func s03(xs []*T) { for _, v := range xs { sink(&v.f) } }  // silent: &v.f, v pointer
func s04(xs []int) *int { for _, v := range xs { return &v }; return nil }  // silent: return &v
func s05(xs []T) *int { for _, v := range xs { return &v.f }; return nil }  // silent: return &v.f
func s06(xs []int) *int { for _, v := range xs { return take(&v) }; return nil }  // FIRES: return f(&v)
func s07(xs []int) { for i := range xs { sink(&i) } }  // silent: key only
func s08(xs []int) { var v int; for _, v = range xs { sink(&v) } }  // FIRES: assign form
func s09(xs []int) { var v int; for _, v = range xs { }; sink(&v) }  // silent: &v after the loop
func s10(xs, ys []int) { for _, a := range xs { for _, b := range ys { _ = b }; sink(&a) } }  // FIRES: after inner loop
func s11(xs []int) { for _, v := range xs { func() { sink(&v) }() } }  // FIRES: closure
func s12(xs []*int) { for _, p := range xs { sink(&p) } }  // FIRES: pointer, no selector
func s13(xs []T) { for _, v := range xs { sink(&T{f: v.f}) } }  // silent: composite literal
func s14(xs []*T) { for _, v := range xs { sink(&v.g.h) } }  // silent: nested selector, pointer
func s15(xs []T) { for _, v := range xs { sink(&v.g.h) } }  // FIRES: nested selector, struct
func s16(xs []P) { for _, v := range xs { sink(&v.f) } }  // FIRES: named pointer
func s17(xs []int) { for k, v := range xs { sink(&k); _ = v } }  // silent: &key with value
func s18(m map[string]int) { for _, v := range m { sink(&v) } }  // FIRES: map
func s19(xs []int) { for _, v := range xs { sink(&(v)) } }  // silent: paren
func s20[E any](xs []E) { for _, v := range xs { sink(&v) } }  // FIRES: generic
func s21(xs, ys []int) { var v int; for _, a := range xs { for _, v = range ys { }; sink(&v); _ = a } }  // FIRES: assign-form inner, &v inside outer
func s22(xs []int) { for _, v := range xs { x := v; sink(&x) } }  // silent: copy
func s23(ch chan int) { for v := range ch { sink(&v) } }  // silent: chan: v is the Key
func s24(s string) { for _, r := range s { sink(&r) } }  // FIRES: string
func s25(xs []int) { for _, v := range xs { if v > 0 { sink(&v) } } }  // FIRES: nested block
func s26(xs []int) { for _, v := range xs { defer sink(&v) } }  // FIRES: defer
func s27(xs []int) { for _, v := range xs { go sink(&v) } }  // FIRES: go
func s28(xs []int) (r *int) { for _, v := range xs { r = &v }; return }  // FIRES: assignment
func s29(xs []A) { for _, v := range xs { sink(&v.f) } }  // FIRES: alias of a pointer
func s30[Q interface{ *T }](xs []Q) { for _, v := range xs { sink(&v) } }  // FIRES: type param, no selector
func s31(xs []int) { for _, v := range xs { _ = -v; sink(&v) } }  // FIRES: another unary first
func s32(xs, ys []int) { for _, v := range xs { _ = v }; for _, w := range ys { sink(&w) } }  // FIRES: second loop
