package main

import (
	"crypto/aes"
	"crypto/cipher"
)

func main() {
	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	// NewCBCEncrypter SHOULD be flagged - encryption should use random nonce
	aesCBC := cipher.NewCBCEncrypter(block, []byte("ILoveMyNonceAlot"))
	var output = make([]byte, 16)
	aesCBC.CryptBlocks(output, []byte("Very Cool thing!"))
}
