package secondfails

import "embed"

//go:embed have.txt nope.txt
var asset embed.FS

// Asset fails on the *second* pattern, and the position is that pattern's.
func Asset() embed.FS { return asset }
