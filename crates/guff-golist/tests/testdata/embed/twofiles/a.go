package twofiles

import "embed"

//go:embed shared
var first embed.FS

// First holds the occurrence the error is reported at: `EmbedPatternPos[p][0]`
// is the first in file-scan order, and files are visited sorted by name.
func First() embed.FS { return first }
