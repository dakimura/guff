// Package dotimportsallowed has two dot imports. With
// `allowedPackages: [example.com/revive/dot]` only the second is reported.
package dotimportsallowed

import (
	. "example.com/revive/dot"
	. "example.com/revive/dotother"
)

var _ = X + Y
