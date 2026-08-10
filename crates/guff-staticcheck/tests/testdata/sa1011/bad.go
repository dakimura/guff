package main

import "strings"

func main() {
	// A Go string is bytes, so "\xff" is a single 0xFF byte and not valid
	// UTF-8. While guff decoded the escape into the code point U+00FF, every
	// constant reaching this check was valid UTF-8 by construction and SA1011
	// could not fire at all.
	strings.Trim("foo", "\xff")
	strings.ContainsAny("bar", "\x80")
	strings.IndexAny("baz", "a\xffb")
	strings.LastIndexAny("qux", "\xed\xa0\x80")
	// A truncated sequence is ill-formed too, even though every byte in it is
	// a legal UTF-8 byte somewhere else.
	strings.TrimRight("quux", "\xe2\x98")
	// Two ill-formed bytes are two U+FFFD runes, so SA1024 also fires here.
	strings.TrimLeft("corge", "\xff\xfe")
}
