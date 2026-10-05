package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
)

func unsafeOverwrite(i int) {
	iv := make([]byte, 16)
	rand.Read(iv)
	if i > 0 {
		iv[i % 16] = 0
	}
	block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
	_ = cipher.NewCTR(block, iv)
}
