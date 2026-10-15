// File version is 1.21.
//
// The module is within the forward compatibility regime so
// the build tag (1.16) can modify the file version, but it cannot
// go below the 1.21 "event horizon" (#68658).

//go:build go1.16

package mod21

import "bytes"
import "go/types"

var _ = bytes.Clone
var _ types.Alias // want `types.Alias requires go1.22 or later \(module is go1.21\)`

