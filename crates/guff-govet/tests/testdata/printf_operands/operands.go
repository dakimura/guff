// Operand shapes for x/tools v0.50's printf matcher that its own testdata
// covers only in a/a.go (not in the golden case: a/ needs cross-package
// wrapper facts): pointers and unsafe.Pointer under integer verbs, which
// carry "(use %p for a pointer)" at any depth; nested pointers to
// composites; maps, funcs and slices that guff used to accept for any verb
// with a pointer bit; untyped nil; and go1.26's %q, which takes only rune
// and byte.
package operands

import (
	"fmt"
	"unsafe"
)

type S struct{ A, B int }
type P struct{ X *int }

func f(p unsafe.Pointer, ip *int, sp *S, u uintptr, pp *P, ps []*int, n int) {
	fmt.Printf("%c", p)
	fmt.Printf("%U", p)
	fmt.Printf("%d", p)
	fmt.Printf("%x", p)
	fmt.Printf("%p", p)
	fmt.Printf("%s", p)
	fmt.Printf("%t", p)
	fmt.Printf("%q", p)
	fmt.Printf("%c", ip)
	fmt.Printf("%U", ip)
	fmt.Printf("%s", ip)
	fmt.Printf("%q", ip)
	fmt.Printf("%c", sp)
	fmt.Printf("%c", u)
	fmt.Printf("%c", pp)
	fmt.Printf("%c", ps)
	fmt.Printf("%*d", ip, n)
	fmt.Printf("%*d", p, n)
	fmt.Printf("%.*d", ip, n)
	fmt.Printf("%c", map[int]*int{})
	fmt.Printf("%c", [2]*int{})
	fmt.Printf("%c", struct{ X *int }{})
}

type R int32
type B uint8

func g(r rune, b byte, i int, i64 int64, rr R, bb B, ps []*S) {
	fmt.Printf("%q", r)
	fmt.Printf("%q", b)
	fmt.Printf("%q", i)
	fmt.Printf("%q", i64)
	fmt.Printf("%q", rr)
	fmt.Printf("%q", bb)
	fmt.Printf("%q", 65)
	fmt.Printf("%q", 'a')
	fmt.Printf("%q", []int{1})
	fmt.Printf("%q", []rune{1})
	fmt.Printf("%d", nil)
	fmt.Printf("%d", ps)
	fmt.Printf("%d", &ps)
	fmt.Printf("%c", i)
}

func h(ps []*S, pi []*int, ms map[string]int, mi map[int]int, fn func(), ss []string, ch chan int) {
	fmt.Printf("%x", ps)
	fmt.Printf("%x", pi)
	fmt.Printf("%d", pi)
	fmt.Printf("%d", ms)
	fmt.Printf("%d", mi)
	fmt.Printf("%x", ms)
	fmt.Printf("%d", fn)
	fmt.Printf("%x", fn)
	fmt.Printf("%p", fn)
	fmt.Printf("%d", ss)
	fmt.Printf("%x", ss)
	fmt.Printf("%p", ss)
	fmt.Printf("%p", ms)
	fmt.Printf("%d", ch)
	fmt.Printf("%b", ss)
	fmt.Printf("%o", [2]*S{})
}
