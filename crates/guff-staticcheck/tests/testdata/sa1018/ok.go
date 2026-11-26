package main

import "strings"

func main() {
	strings.Replace("abc", "a", "b", -1)
	strings.Replace("abc", "a", "b", 1)
}
