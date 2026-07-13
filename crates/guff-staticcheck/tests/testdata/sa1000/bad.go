package main

import "regexp"

func main() {
	regexp.Compile(`foo(`)
	regexp.MatchString("foo(", "")
	regexp.MustCompile(`[`)
}
