package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesGCM, _ := cipher.NewGCM(block)
	cipheredText := aesGCM.Seal(nil, func() []byte { return []byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1} }(), []byte("My secret message"), nil)
	fmt.Println(string(cipheredText))
	cipheredText, _ = aesGCM.Open(nil, func() []byte { return []byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1} }(), cipheredText, nil)
	fmt.Println(string(cipheredText))

}
