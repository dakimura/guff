package allmissing

import "embed"

//go:embed all:hidden
var asset embed.FS

// Asset pins that the `all:` prefix stays part of the reported pattern.
func Asset() embed.FS { return asset }
