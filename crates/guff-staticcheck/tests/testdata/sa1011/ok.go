package main

import "strings"

func main() {
	strings.Trim("foo", "bar")
	strings.ContainsAny("baz", "abc")
	strings.IndexAny("x", "xyz")
}
