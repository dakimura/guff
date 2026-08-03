package main

import "regexp"

func f(s string) { regexp.MatchString("a", s) }

// Not in a loop — sequential if-init MatchString must not be flagged
// (natural-loop detection; straight-line preds must not count as back-edges).
func sequential(lines []string) {
	if match, _ := regexp.MatchString(`a`, lines[0]); !match {
		return
	}
	if match, _ := regexp.MatchString(`b`, lines[1]); !match {
		return
	}
}
