package pkg

import (
	"bytes"
	"strings"
)

func fn() {
	strings.ReplaceAll("", "", "")
	strings.Replace("", "", "", 1)
	strings.Split("", "")
	bytes.ReplaceAll(nil, nil, nil)
}
