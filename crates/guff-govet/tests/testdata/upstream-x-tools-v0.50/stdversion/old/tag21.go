// The build tag is ignored due to the module version.

//go:build go1.21

package old

import "go/types"

var _ types.Alias // no diagnostic: go.mod is too old for us to care
