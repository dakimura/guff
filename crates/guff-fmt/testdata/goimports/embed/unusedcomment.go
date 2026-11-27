package unusedcomment

import (
	"embed" // for go:embed
	"fmt"
)

//go:embed data.txt
var s string

func F() { fmt.Println(s) }
