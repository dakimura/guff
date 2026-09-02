package twofiles

import "embed"

//go:embed shared
var second embed.FS

// Second repeats the pattern; the pattern list is deduplicated.
func Second() embed.FS { return second }
