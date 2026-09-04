package depr

import "example.com/sa1019promoted/dep"

func Direct(b *dep.Base) { b.Old = "x" }

func PromotedValue(w *dep.Wrapper) { w.Old = "x" }

func PromotedPtr(w *dep.PtrWrapper) { w.Old = "x" }

func PromotedRead(w dep.Wrapper) string { return w.Old }

// Two levels of embedding, and the field is declared in a third package that
// this file does not import.
func PromotedTwoLevelsCrossPkg(o *dep.Outer) { o.DeepOld = "x" }

func NamedThenPromoted(h *dep.Holder) { h.Cfg.Old = "x" }

func ExplicitPath(w *dep.Wrapper) { w.Base.Old = "x" }

// Controls: live siblings at each depth, and a same-named live field.
func SiblingIsFine(w *dep.Wrapper) { w.New = "x"; w.Extra = "y" }

func DeepSiblingIsFine(o *dep.Outer) { o.Fine = "x" }

func OtherOldIsFine(o *dep.Other) { o.Old = "x" }
