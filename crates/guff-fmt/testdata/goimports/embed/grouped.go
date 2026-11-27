package grouped

import (
	"fmt"
	"os"

	"strings"
)

//go:embed data.txt
var s string

func F() { fmt.Println(s, os.Args, strings.ToUpper(s)) }
