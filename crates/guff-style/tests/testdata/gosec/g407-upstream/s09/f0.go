package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesGCM, _ := cipher.NewGCM(block)

	cipherText := aesGCM.Seal(nil, []byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1}, []byte("My secret message"), nil)
	fmt.Println(string(cipherText))

	cipherText, _ = aesGCM.Open(nil, []byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1}, cipherText, nil)
	fmt.Println(string(cipherText))
}
