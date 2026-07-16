package mirror

import (
	"strings"
	"unicode/utf8"
)

func bad() {
	_ = strings.Compare(string([]byte{'f', 'o', 'o'}), string([]byte{'b', 'a', 'r'}))
	_ = utf8.RuneCount([]byte("foobar"))
	_ = strings.Contains(string([]byte("x")), string([]byte("y")))
}
