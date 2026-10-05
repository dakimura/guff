package stringscut

import (
	"bytes"
	"strings"
)

func splitFirst(s string) string {
	x := strings.Split(s, ",")[0]
	return x
}

func splitNFirst(s string) string {
	x := strings.SplitN(s, "=", 2)[0]
	return x
}

func bytesSplitFirst(b []byte) []byte {
	x := bytes.Split(b, []byte(","))[0]
	return x
}

func bytesSplitNFirst(b []byte) []byte {
	x := bytes.SplitN(b, []byte("="), 2)[0]
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

// Each condition of `stringsplitCut` (x/tools v0.50), one shape apiece.
func splitAssign(s string) string {
	var x string
	x = strings.Split(s, ",")[0] // `=`, not `:=`
	return x
}

func splitBlank(s string) {
	_ = strings.Split(s, ",")[0]
}

func splitSecond(s string) string {
	x := strings.Split(s, ",")[1] // index 1
	return x
}

func splitNThree(s string) string {
	x := strings.SplitN(s, ",", 3)[0] // n is 3
	return x
}

func splitParen(s string) string {
	x := (strings.Split(s, ","))[0] // the call is parenthesised
	return x
}

const comma = ","

func splitNamedSep(s string) string {
	x := strings.Split(s, comma)[0] // a named constant is still a constant
	return x
}
