package glob

import "embed"

//go:embed *.tmpl
var asset embed.FS

// Asset pins that a glob matching nothing fails like a literal one.
func Asset() embed.FS { return asset }
