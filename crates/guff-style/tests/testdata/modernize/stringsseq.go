//go:build go1.24

package modernize

import (
	"bytes"
	"strings"
)

func rangeSplit(s string) {
	for _, part := range strings.Split(s, ",") {
		_ = part
	}
}

func rangeFields(s string) {
	for _, part := range strings.Fields(s) {
		_ = part
	}
}

// `bytes.Split` and `bytes.Fields` are the other half of the same four-function
// table upstream looks up; `bytes` grew `SplitSeq`/`FieldsSeq` in the same
// release, and the analyzer's doc comment lists them. syncthing
// `cmd/syncthing/crash_reporting.go` ranges over `bytes.Split(data, …)`.
func rangeBytesSplit(data []byte) {
	for _, line := range bytes.Split(data, []byte("\n")) {
		_ = line
	}
}

func rangeBytesFields(data []byte) {
	for _, f := range bytes.Fields(data) {
		_ = f
	}
}

// silent — `SplitN` is not one of the four.
func rangeSplitN(s string) {
	for _, part := range strings.SplitN(s, ",", 2) {
		_ = part
	}
}
