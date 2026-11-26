package gocheckcompilerdirectives

import _ "embed"

//go:generate echo hello world

//go:embed
var Value string

// go:

//go:noinline
func notInlined() {}

//go:fix inline
func inlined() int {
	return 6
}

// regular comment mentioning go:embed is fine
func ok() {}
