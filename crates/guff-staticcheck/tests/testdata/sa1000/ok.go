package main

import "regexp"

func main() {
	regexp.MustCompile(`(abc)`)
	regexp.Compile(`ok`)
}
