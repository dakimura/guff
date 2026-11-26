package main

import "regexp"

func main() {
	var r *regexp.Regexp
	var b []byte
	_ = r.FindAll(b, 0)
	regexp.MustCompile("a").FindAll(b, 0)
}
