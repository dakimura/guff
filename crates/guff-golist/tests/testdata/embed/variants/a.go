package variants

import "embed"

//go:embed prodmissing
var prod embed.FS

// Prod is the production embed; its error also lands on `P [P.test]`.
func Prod() embed.FS { return prod }
