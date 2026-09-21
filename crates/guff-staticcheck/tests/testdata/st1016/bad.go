package pkg

type T1 int

func (x T1) Fn1()    {}
func (y T1) Fn2()    {}
func (x T1) Fn3()    {}
func (T1) Fn4()      {}
func (_ T1) Fn5()    {}
func (self T1) Fn6() {}

type T3 struct{}

func (bar T3) Fn2()  {}
func (meow T3) Fn3() {}

type T4 struct{}

func (bar T4) Fn2() {}

// Ordered pins where the diagnostic lands. Upstream reports at `firstFn`, the
// first entry of `IntuitiveMethodSet` — ordered like `types.MethodSet`, i.e.
// sorted by `obj.Id()`, not by source position. T1 above cannot show that: its
// first method is also its alphabetically first.
type Ordered struct{}

func (z Ordered) Zeta()   {}
func (a Ordered) Apex()   {}
func (m Ordered) Middle() {}

// BlankWinsThePosition has its `firstFn` on a method with an unnamed receiver.
// Upstream sets `firstFn` before it looks at the name, so the method that
// carries the diagnostic can be one that contributes nothing to `seen`.
type BlankWinsThePosition struct{}

func (b BlankWinsThePosition) Zeta() {}
func (BlankWinsThePosition) Apex()   {}
func (c BlankWinsThePosition) Mid()  {}

// UnderscoreWinsThePosition is the same with `_`.
type UnderscoreWinsThePosition struct{}

func (u UnderscoreWinsThePosition) Zeta() {}
func (_ UnderscoreWinsThePosition) Apex() {}
func (v UnderscoreWinsThePosition) Mid()  {}

// IDBeatsBareName sorts by `Id`, which qualifies an unexported name with the
// package path — so `Zeta` comes before `<pkgpath>.apex`, the opposite of what
// the bare names say.
type IDBeatsBareName struct{}

func (e IDBeatsBareName) Zeta() {}
func (f IDBeatsBareName) apex() {}

// MixedReceivers puts value and pointer receivers in one method set.
type MixedReceivers struct{}

func (m MixedReceivers) Zeta()    {}
func (n *MixedReceivers) Apex()   {}
func (o *MixedReceivers) Middle() {}
