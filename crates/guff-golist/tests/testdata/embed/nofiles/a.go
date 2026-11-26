package nofiles

import "embed"

//go:embed only
var asset embed.FS

// Asset names a directory that exists but holds nothing embeddable — a
// different message from "no matching files found".
func Asset() embed.FS { return asset }
