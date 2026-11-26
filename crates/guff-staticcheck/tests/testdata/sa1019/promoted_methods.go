package main

import "example.com/oldmethod"

// Ten ways to reach a deprecated method, measured against golangci-lint
// 2.12.2 (staticcheck alone): nine are reported, one is silent — and
// `localPromoted` below is an eleventh, silent. Each comment says which.
func promotedMethods(n oldmethod.Node) {
	_ = (&oldmethod.Base{}).Text()   // reported: declared on the operand's type
	_ = (&oldmethod.Inline{}).Text() // reported: promoted one level
	_ = (&oldmethod.Deep{}).Text()   // reported: promoted two levels (goldmark's shape)
	_ = (&oldmethod.PtrEmb{}).Text() // reported: promoted through an embedded pointer
	_ = (&oldmethod.Own{}).Text()    // silent: shadowed by a live method
	_ = oldmethod.Inline{}.Val()     // reported: value receiver, promoted
	_ = n.Text()                     // reported: interface method
	x := &oldmethod.Deep{}
	_ = x.Text                       // reported: method value, promoted
	(&oldmethod.Wrap{}).Old()        // reported: declared in a package this file does not import
	_ = (&oldmethod.Own{}).Val()     // reported: Own shadows Text, not Val
}

// localPromoted: SA1019 lets a package use its own deprecated objects.
type localBase struct{}

// Deprecated: local.
func (localBase) M() {}

type localInline struct{ localBase }

func localPromoted() { localInline{}.M() } // silent: same package

func main() {
	promotedMethods(nil)
	localPromoted()
}
