package main

import (
	"crypto/aes"
	"crypto/cipher"
)

var globalIV = []byte("1234567812345678")

func wrapper(iv []byte, b cipher.Block) {
	cipher.NewCTR(b, iv)
}

func main() {
	b, _ := aes.NewCipher([]byte("1234567812345678"))
	wrapper(globalIV, b)
}
