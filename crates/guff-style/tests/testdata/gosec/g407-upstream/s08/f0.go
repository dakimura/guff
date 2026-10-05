package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesGCM, _ := cipher.NewGCM(block)
	cipherText := aesGCM.Seal(nil, []byte{}, []byte("My secret message"), nil)
	fmt.Println(string(cipherText))

	cipherText, _ = aesGCM.Open(nil, []byte{}, cipherText, nil)
	fmt.Println(string(cipherText))
}
