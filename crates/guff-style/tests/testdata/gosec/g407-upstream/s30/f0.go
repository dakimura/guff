package main

import (
	"crypto/aes"
	"crypto/cipher"
)

func fill(b []byte) {
	b[0] = 1
}

func main() {
	iv := make([]byte, 16)
	fill(iv)
	block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
	_ = cipher.NewCTR(block, iv)
}
