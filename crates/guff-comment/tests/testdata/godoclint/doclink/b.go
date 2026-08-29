package doclink

import (
	blah "encoding/json"
	"os"
)

var _ = blah.Encoder{}
var _ = os.Args

// Kilo refers to blah.Encoder, the alias the file actually imports
// encoding/json as. Upstream resolves the alias to the import path before
// looking the symbol up.
const Kilo = 0

// Lima refers to os.Args by the package's own name.
const Lima = 0
