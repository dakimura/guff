package main

import "regexp"

func main() {
	var r *regexp.Regexp
	var b []byte
	_ = r.FindAll(b, -1)
	_ = r.FindAll(b, 1)
	regexp.MustCompile("a").FindAll(b, -1)
}
