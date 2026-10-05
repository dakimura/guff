package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
)

func main() {
	buf := make([]byte, 128)
	rand.Read(buf[32:48])
	block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
	_ = cipher.NewCTR(block, buf[32:48])
}
