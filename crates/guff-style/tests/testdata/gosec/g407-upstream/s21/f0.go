package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

var nonce = []byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1}

func main() {

	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesGCM, _ := cipher.NewGCM(block)
	cipherText := aesGCM.Seal(nil, nonce, []byte("My secret message"), nil)
	fmt.Println(string(cipherText))

}
