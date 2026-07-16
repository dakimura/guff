//go:build go1.24

package modernize

import "strings"

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
