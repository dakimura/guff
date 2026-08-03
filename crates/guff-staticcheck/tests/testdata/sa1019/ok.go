package main

import "example.com/old"

// Embedding a deprecated type must not flag SA1019 (go/types ObjectOf returns
// the field Var for the embedded type Ident, not the TypeName).
type Embedder struct {
	old.OldClient
}

func main() {}
