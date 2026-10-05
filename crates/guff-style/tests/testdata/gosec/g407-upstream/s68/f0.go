package main

import (
	"crypto/aes"
	"crypto/cipher"
)

func main() {
	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesGCM, _ := cipher.NewGCM(block)

	// Encrypt with hardcoded nonce - SHOULD be flagged
	cipherText := aesGCM.Seal(nil, []byte("ILoveMyNonce"), []byte("My secret message"), nil)

	// Decrypt with same nonce - should NOT be flagged (same nonce as encryption)
	cipherText, _ = aesGCM.Open(nil, []byte("ILoveMyNonce"), cipherText, nil)
}
