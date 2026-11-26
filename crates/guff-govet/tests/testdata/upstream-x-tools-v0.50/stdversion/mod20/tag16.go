// The 1.20 module is before the forward compatibility regime:
// The file's build tag effects selection, but
// not language semantics, so stdversion is silent.

//go:build go1.16

package mod20

import "bytes"
import "go/types"

var _ = bytes.Clone
var _ types.Alias

