package main

import (
	"crypto/aes"
	"crypto/cipher"
)

func main() {
	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	// NewCFBEncrypter SHOULD be flagged - encryption should use random nonce
	aesCFB := cipher.NewCFBEncrypter(block, []byte("ILoveMyNonceAlot"))
	var output = make([]byte, 16)
	aesCFB.XORKeyStream(output, []byte("Very Cool thing!"))
}
