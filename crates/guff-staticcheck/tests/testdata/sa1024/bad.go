package main

import "strings"

func main() {
	strings.TrimLeft("foo", "aba")
	strings.TrimRight("bar", "121")
	strings.Trim("baz", "xyx")
}
