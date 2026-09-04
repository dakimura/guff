package gocritic

import (
	"bytes"
	gotypes "go/types"
	"image"
	"image/draw"
	"strings"
)

// dupArg, both halves of the rule.
//
//	m.Match(`$x.Equal($x)`, `$x.Equals($x)`, `$x.Compare($x)`, `$x.Cmp($x)`).
//		Where(m["x"].Pure).
//		Report(`suspicious method call with the same argument and receiver`)
//
//	m.Match(`copy($x, $x)`, … , `strings.Replace($_, $x, $x, $_)`,
//		`draw.Draw($x, $_, $x, $_, $_)`).
//		Where(m["x"].Pure).
//		Report(`suspicious duplicated args in $$`)
//
// Two things the second half is easy to get wrong: three of its patterns do
// **not** compare arguments 0 and 1, and the qualifier is resolved to an import
// **path**, not matched by spelling — an aliased `gotypes "go/types"` is still
// reported, and a local package that happens to be named `strings` is not
// (measured; that shape needs a second package, so it lives in
// docs/COMPAT-HARDENING.md rather than here).

type dupArgSeg struct{ v int }

func (s *dupArgSeg) Equal(o *dupArgSeg) bool  { return s.v == o.v }
func (s *dupArgSeg) Equals(o *dupArgSeg) bool { return s.v == o.v }
func (s *dupArgSeg) Compare(o *dupArgSeg) int { return s.v - o.v }
func (s *dupArgSeg) Cmp(o ...*dupArgSeg) int  { return s.v }
func (s *dupArgSeg) Same(o *dupArgSeg) bool   { return s.v == o.v }
func (s *dupArgSeg) Eq2(a, b *dupArgSeg) bool { return a == b }

type dupArgHolder struct{ s *dupArgSeg }

type dupArgFieldHolder struct {
	Equal func(o dupArgFieldHolder) bool
}

type dupArgBox[X any] struct{}

func (dupArgBox[X]) Equal(o dupArgBox[X]) bool { return true }

func dupArgNew() *dupArgSeg { return &dupArgSeg{} }

// Every call here is a finding.
func dupArgMethodBad(
	a *dupArgSeg,
	h dupArgHolder,
	arr []*dupArgSeg,
	m map[string]*dupArgSeg,
	fh dupArgFieldHolder,
	b dupArgBox[int],
) {
	_ = a.Equal(a)
	_ = a.Equals(a)
	_ = a.Compare(a)
	_ = a.Cmp(a)
	_ = h.s.Equal(h.s)
	_ = arr[0].Equal(arr[0])
	_ = m["k"].Equal(m["k"])
	_ = (a).Compare((a))
	_ = b.Equal(b)
	// gogrep is syntactic: `Equal` here is a *field* of function type, not a
	// method, and it is still a match.
	_ = fh.Equal(fh)
}

// Every call here is silent.
func dupArgMethodOK(a *dupArgSeg, ch chan *dupArgSeg) {
	_ = a.Same(a)                      // not one of the four names
	_ = a.Eq2(a, a)                    // two arguments; the pattern fixes the arity at one
	_ = a.Cmp(a, a)                    // the same, on a variadic method
	_ = a.Equal(dupArgNew())           // the receiver and the argument differ
	_ = dupArgNew().Equal(dupArgNew()) // a call is not Pure
	_ = (<-ch).Compare(<-ch)           // nor is a channel receive
}

// The names guff did not know, one call each. `Replace` and `ReplaceAll`
// compare arguments 1 and 2; `draw.Draw` compares 0 and 2.
func dupArgCallsBad(s string, b []byte, n int, t gotypes.Type) {
	_ = strings.LastIndex(s, s)
	_ = strings.Split(s, s)
	_ = strings.SplitAfter(s, s)
	_ = strings.SplitAfterN(s, s, n)
	_ = strings.SplitN(s, s, n)
	_ = strings.Replace("z", s, s, n)
	_ = strings.ReplaceAll("z", s, s)
	_ = bytes.LastIndex(b, b)
	_ = bytes.Split(b, b)
	_ = bytes.SplitAfter(b, b)
	_ = bytes.SplitAfterN(b, b, n)
	_ = bytes.SplitN(b, b, n)
	_ = bytes.Replace(nil, b, b, n)
	_ = bytes.ReplaceAll(nil, b, b)
	_ = gotypes.Identical(t, t)
	_ = gotypes.IdenticalIgnoreTags(t, t)
}

func dupArgDrawBad(dst draw.Image, r image.Rectangle, p image.Point) {
	draw.Draw(dst, r, dst, p, draw.Src)
}

func dupArgImpure() string { return "" }

func dupArgCallsOK(s string, n int) {
	// `$x` is not Pure.
	_ = strings.Contains(dupArgImpure(), dupArgImpure())
	_ = strings.Replace(s, dupArgImpure(), dupArgImpure(), n)
	// Pure applies to `$x` only, so an impure `$_` hole is still a finding —
	// see `dupArgCallsBad`. Here arguments 0 and 1 are equal but the pattern
	// compares 1 and 2.
	_ = strings.Replace("a", "a", s, n)
	// A slice expression is Pure for `typep.SideEffectFree` but not for
	// ruleguard's `isPure`, which every `Where(… .Pure)` is written against.
	_ = strings.Contains(s[:1], s[:1])
}
