package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
)

func main() {
	key := []byte("example key 1234")
	block, _ := aes.NewCipher(key)
	iv := []byte("1234567890123456")
	iv[8] = 0
	rand.Read(iv)
	stream := cipher.NewCTR(block, iv)
	_ = stream
}
