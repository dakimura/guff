package mirror

import (
	"strings"
	"unicode/utf8"
)

func ok() {
	_ = strings.Compare("foo", "bar")
	_ = utf8.RuneCountInString("foobar")
	_ = strings.Contains("x", "y")
	_ = strings.Compare(string([]byte{'f'}), "bar") // only one arg converted
}
