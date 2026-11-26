package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
)

func safeOverwrite(i int) {
	iv := make([]byte, 128)
	rand.Read(iv)
	if i >= 16 && i < 128{
		iv[i] = 0
	}
	block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
	_ = cipher.NewCTR(block, iv[:16])
}
