package ok

import "embed"

//go:embed data
var asset embed.FS

// Asset resolves: a directory pattern takes the tree below it, minus the
// dot-names, which `all:` would keep.
func Asset() embed.FS { return asset }
