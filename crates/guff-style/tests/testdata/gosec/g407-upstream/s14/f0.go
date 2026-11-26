package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesCFB := cipher.NewCFBEncrypter(block, []byte("ILoveMyNonceAlot"))
	var output = make([]byte, 16)
	aesCFB.XORKeyStream(output, []byte("Very Cool thing!"))
	fmt.Println(string(output))
	aesCFB = cipher.NewCFBDecrypter(block, []byte("ILoveMyNonceAlot"))
	aesCFB.XORKeyStream(output, output)
	fmt.Println(string(output))

}