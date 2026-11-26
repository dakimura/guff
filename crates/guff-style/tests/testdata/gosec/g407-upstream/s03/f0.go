package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	block, _ := aes.NewCipher( []byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesGCM, _ := cipher.NewGCM(block)

	cipherText := aesGCM.Seal(nil, func() []byte {
		if true {
			return []byte("ILoveMyNonce")
		} else {
			return []byte("IDont'Love..")
		}
	}(), []byte("My secret message"), nil) // #nosec G407
	fmt.Println(string(cipherText))

	cipherText, _ = aesGCM.Open(nil, func() []byte {
		if true {
			return []byte("ILoveMyNonce")
		} else {
			return []byte("IDont'Love..")
		}
	}(), cipherText, nil) // #nosec G407

	fmt.Println(string(cipherText))
}
