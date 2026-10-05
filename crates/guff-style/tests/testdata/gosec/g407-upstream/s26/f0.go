package main

import (
	"crypto/aes"
	"crypto/cipher"
)

func main() {
	k := make([]byte, 48)
	key, iv := k[:32], k[32:]
	block, _ := aes.NewCipher(key)
	_ = cipher.NewCTR(block, iv)
}
