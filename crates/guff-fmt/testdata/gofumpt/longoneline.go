// Package longoneline pins gofumpt's decl-separation rule to the version
// golangci-lint vendors.
//
// v0.10.0 counts a single-source-line func whose printed form exceeds 100 bytes
// as multi-line, and puts a blank line between it and its multi-line
// neighbours. v0.9.2 — the pin — does not, so this file is already formatted.
// dapr `pkg/actors/table/fake/fake.go` is exactly this shape.
package longoneline

// Fake is a hand-written test double, the shape that produces these.
type Fake struct {
	getOrCreateFn  func(string, string) bool
	actorExistsFn  func(string, string) bool
	registerTypeFn func(string)
}

func (f *Fake) WithGetOrCreate(fn func(string, string) bool) *Fake {
	f.getOrCreateFn = fn
	return f
}
func (f *Fake) WithActorExists(fn func(string, string) bool) *Fake { f.actorExistsFn = fn; return f }
func (f *Fake) WithRegisterType(fn func(string)) *Fake {
	f.registerTypeFn = fn
	return f
}
