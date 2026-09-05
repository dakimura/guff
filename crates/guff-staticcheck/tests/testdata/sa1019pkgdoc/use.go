package use

import (
	"example.com/sa1019pkgdoc/dep"
	"example.com/sa1019pkgdoc/live"
)

// The import of a deprecated package is the finding; the uses below are not.
var _ = dep.New()
var _ = dep.Other()

var _ = live.Fine()
var _ = live.Extra()
