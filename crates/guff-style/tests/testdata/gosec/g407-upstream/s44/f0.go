package main

import (
	"crypto/aes"
	"crypto/cipher"
)

func test(init func([]byte)) {
	key := []byte("example key 1234")
	block, _ := aes.NewCipher(key)
	iv := make([]byte, 16)
	init(iv) // We can't resolve 'init', should default to Dynamic to avoid FP
	stream := cipher.NewCTR(block, iv)
	_ = stream
}
