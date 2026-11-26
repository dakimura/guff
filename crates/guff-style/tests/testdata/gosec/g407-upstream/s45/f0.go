package main

import (
	"crypto/aes"
	"crypto/cipher"
	"io"
)

type CustomReader interface {
	io.Reader
}

func testCustomReader(cr CustomReader) {
	key := []byte("example key 1234")
	block, _ := aes.NewCipher(key)
	iv := make([]byte, 16)
	cr.Read(iv)
	stream := cipher.NewCTR(block, iv)
	_ = stream
}
