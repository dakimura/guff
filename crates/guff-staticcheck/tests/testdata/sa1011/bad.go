package main

import "strings"

func main() {
	// SC-D08: `\xNN` with NN>=0x80 may not reproduce Go's raw-byte string constants
	// in guff; integration coverage defers to unit tests for byte-level UTF-8.
	strings.Trim("foo", "\xff")
	strings.ContainsAny("bar", "\x80")
}
