package main

import "strings"

func f(s, prefix string) {
	if strings.HasPrefix(s, prefix) {
		s = s
	}
}
