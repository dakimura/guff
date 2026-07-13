package main

import "bytes"

func f(a, b []byte) bool {
	return bytes.Equal(a, b)
}
