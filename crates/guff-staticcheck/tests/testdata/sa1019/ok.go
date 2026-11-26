package main

import "example.com/old"

// Embedding a deprecated type must not flag SA1019 (go/types ObjectOf returns
// the field Var for the embedded type Ident, not the TypeName).
type Embedder struct {
	old.OldClient
}

func main() {
	// Live siblings of a deprecated field, promoted the same way, must stay
	// quiet — so must a same-named field on an unrelated type.
	var w old.Wrapper
	_ = w.Fine
	_ = w.Extra

	var h old.Holder
	_ = h.Cfg.Fine

	var other old.Other
	_ = other.Old
}
