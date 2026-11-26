package main

import "strings"

func main() {
	strings.Trim("foo", "bar")
	strings.ContainsAny("baz", "abc")
	strings.IndexAny("x", "xyz")
	// The same code points spelled as well-formed UTF-8. `ÿ` is two
	// bytes, `\xff` is one; only the latter is invalid.
	strings.TrimLeft("y", "ÿ☃")
	strings.TrimRight("z", "é")
}
