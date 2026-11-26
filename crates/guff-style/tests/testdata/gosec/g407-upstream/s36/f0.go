package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"os"
)

func main() {
	key := []byte("example key 1234")
	block, _ := aes.NewCipher(key)
	iv := []byte("1234567890123456")
	rand.Read(iv)

	var f func(cipher.Block, []byte) cipher.Stream
	if len(os.Args) > 1 {
		f = cipher.NewCTR
	} else {
		f = cipher.NewOFB
	}
	stream := f(block, iv)
	_ = stream
}
