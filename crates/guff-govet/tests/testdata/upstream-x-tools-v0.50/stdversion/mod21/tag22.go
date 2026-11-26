// File version is 1.22.
//
// The module is within the forward compatibility regime so
// the build tag (1.22) updates the file version to 1.22.

//go:build go1.22

package mod21

import "bytes"
import "go/types"

var _ = bytes.Clone
var _ types.Alias 

