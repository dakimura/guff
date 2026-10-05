package main

import (
	"crypto/aes"
	"crypto/cipher"
)

func main() {
	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	// NewCBCDecrypter should not be flagged - decryption must use same nonce as encryption
	aesCBC := cipher.NewCBCDecrypter(block, []byte("ILoveMyNonceAlot"))
	var output = make([]byte, 16)
	aesCBC.CryptBlocks(output, []byte("encrypted_block!"))
}
