package pkg

import (
	"bytes"
	"strings"
)

func fn() {
	strings.Replace("", "", "", -1)
	strings.Replace("", "", "", 0)
	strings.SplitN("", "", -1)
	strings.SplitAfterN("", "", -1)
	bytes.Replace(nil, nil, nil, -1)
	bytes.SplitN(nil, nil, -1)
	bytes.SplitAfterN(nil, nil, -1)
}
