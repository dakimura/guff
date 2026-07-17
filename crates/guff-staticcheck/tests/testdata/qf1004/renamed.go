package pkg

import (
	b "bytes"
	s "strings"
)

func renamed() {
	s.Replace("", "", "", -1)
	b.Replace(nil, nil, nil, -1)
}
