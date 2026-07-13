package main

import (
	"bytes"
	"strings"
)

func f(s string, b []byte) {
	if strings.Index(s, "x") != -1 {
	}
	if strings.Index(s, "x") == -1 {
	}
	if strings.IndexRune(s, 'x') > -1 {
	}
	if bytes.Index(b, []byte("x")) >= 0 {
	}
}
