package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
)

func main() {
	iv := make([]byte, 16)
	rand.Read(iv[6:12])
	iv[6] = 0
	rand.Read(iv[0:7])
	iv[10] = 0
	rand.Read(iv[10:16])
	block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
	_ = cipher.NewCTR(block, iv)
}
