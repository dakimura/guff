package main

import (
	"crypto/aes"
	"crypto/cipher"
	"io"
)

func main() {
	iv := make([]byte, 16)
	io.ReadFull(nil, iv)
	block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
	_ = cipher.NewCTR(block, iv)
}
