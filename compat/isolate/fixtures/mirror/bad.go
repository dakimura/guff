package p

import "strings"

func Bad(s string) bool {
	return strings.Index(s, "x") >= 0
}
