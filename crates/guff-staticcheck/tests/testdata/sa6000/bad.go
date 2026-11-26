package main

import "regexp"

func f(s string) {
	for len(s) >= 0 {
		regexp.MatchString("a", s)
	}
}
