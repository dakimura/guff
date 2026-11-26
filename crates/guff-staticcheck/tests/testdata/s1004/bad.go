package main

import "bytes"

func f(a, b []byte) bool {
	return bytes.Compare(a, b) == 0
}
