package main

import (
	"crypto/aes"
	"crypto/cipher"
	"io"
)

type CustomReader interface {
	io.Reader
}

func testCustomReaderOverwrite(cr CustomReader) {
	key := []byte("example key 1234")
	block, _ := aes.NewCipher(key)
	iv := make([]byte, 16)
	iv[15] = 1 // Taint
	cr.Read(iv) // Cover via embedded interface
	stream := cipher.NewCTR(block, iv)
	_ = stream
}
