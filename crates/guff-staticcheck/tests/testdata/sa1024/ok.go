package main

import "strings"

func main() {
	strings.TrimLeft("foo", "abc")
	strings.TrimRight("bar", "12")
	strings.Trim("baz", "xy")
}
