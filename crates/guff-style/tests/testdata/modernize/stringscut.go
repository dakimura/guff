package stringscut

import "strings"

func splitFirst(s string) string {
	x := strings.Split(s, ",")[0]
	return x
}

func splitNFirst(s string) string {
	x := strings.SplitN(s, "=", 2)[0]
	return x
}

func skipEmptySep(s string) string {
	x := strings.Split(s, "")[0]
	return x
}

func skipVarSep(s, sep string) string {
	x := strings.Split(s, sep)[0]
	return x
}

func alreadyCut(s string) string {
	x, _, _ := strings.Cut(s, ",")
	return x
}
