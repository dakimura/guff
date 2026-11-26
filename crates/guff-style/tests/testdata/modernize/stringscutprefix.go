package modernize

import (
	"bytes"
	"strings"
)

func pattern2TrimPrefix(s, pre string) string {
	if after := strings.TrimPrefix(s, pre); after != s {
		return after
	}
	return s
}

func pattern2TrimSuffix(s, suf string) string {
	if before := strings.TrimSuffix(s, suf); before != s {
		return before
	}
	return s
}

func bytesHasPrefix(b, pre []byte) []byte {
	if bytes.HasPrefix(b, pre) {
		return bytes.TrimPrefix(b, pre)
	}
	return b
}

func bytesHasSuffix(b, suf []byte) []byte {
	if bytes.HasSuffix(b, suf) {
		return bytes.TrimSuffix(b, suf)
	}
	return b
}

func alreadyCutPrefix(s, pre string) string {
	if after, ok := strings.CutPrefix(s, pre); ok {
		return after
	}
	return s
}
