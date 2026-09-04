package depr

import (
	"example.com/sa1019promoted/dep"
	// `inner` is imported only to make it a *direct* dependency of this
	// package. `dep.Outer` reaches `inner.Deep` on its own, but guff builds
	// SA1019's message by reading the declaring package's sources and finds
	// that package through `Package::imports`, which holds direct edges only —
	// so without this line the two-level form is silent. That is a separate,
	// open defect in the package loader (COMPAT-HARDENING 2026-09-04 続き 159),
	// not in the promoted-field lookup this case is about.
	_ "example.com/sa1019promoted/inner"
)

func Direct(b *dep.Base) { b.Old = "x" }

func PromotedValue(w *dep.Wrapper) { w.Old = "x" }

func PromotedPtr(w *dep.PtrWrapper) { w.Old = "x" }

func PromotedRead(w dep.Wrapper) string { return w.Old }

// Two levels of embedding, and the field is declared in a third package.
func PromotedTwoLevelsCrossPkg(o *dep.Outer) { o.DeepOld = "x" }

func NamedThenPromoted(h *dep.Holder) { h.Cfg.Old = "x" }

func ExplicitPath(w *dep.Wrapper) { w.Base.Old = "x" }

// Controls: live siblings at each depth, and a same-named live field.
func SiblingIsFine(w *dep.Wrapper) { w.New = "x"; w.Extra = "y" }

func DeepSiblingIsFine(o *dep.Outer) { o.Fine = "x" }

func OtherOldIsFine(o *dep.Other) { o.Old = "x" }
