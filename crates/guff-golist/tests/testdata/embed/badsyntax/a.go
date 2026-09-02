package badsyntax

import "embed"

//go:embed ../ok
var asset embed.FS

// Asset escapes the package directory, which is a syntax error rather than a
// missing file.
func Asset() embed.FS { return asset }
