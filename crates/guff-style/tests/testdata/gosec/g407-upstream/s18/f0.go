package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	var nonce = []byte("ILoveMyNonce")
	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesGCM, _ := cipher.NewGCM(block)
	fmt.Println(string(aesGCM.Seal(nil, nonce, []byte("My secret message"), nil)))
}
