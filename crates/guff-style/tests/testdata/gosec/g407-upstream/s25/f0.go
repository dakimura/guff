package main

import (
	"crypto/cipher"
)

func recursive(s string, b cipher.Block) {
	recursive(s, b)
	cipher.NewCTR(b, []byte(s))
}

func main() {
	recursive("1234567812345678", nil)
}
