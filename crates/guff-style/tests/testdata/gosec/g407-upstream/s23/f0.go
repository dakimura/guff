package main

import (
	"crypto/aes"
	"crypto/cipher"
)

const iv = "1234567812345678"

func wrapper(s string, b cipher.Block) {
	cipher.NewCTR(b, []byte(s))
}

func main() {
	b, _ := aes.NewCipher([]byte("1234567812345678"))
	wrapper(iv, b)
}
