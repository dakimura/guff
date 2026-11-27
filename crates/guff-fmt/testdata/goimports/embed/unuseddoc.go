package unuseddoc

import (
	// The directive below needs it.
	"embed"
	"os"
)

//go:embed data.txt
var s string

var _ = os.Args
