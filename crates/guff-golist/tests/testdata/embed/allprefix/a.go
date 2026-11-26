package allprefix

import "embed"

//go:embed all:data
var asset embed.FS

// Asset keeps the dot-names the plain form drops.
func Asset() embed.FS { return asset }
