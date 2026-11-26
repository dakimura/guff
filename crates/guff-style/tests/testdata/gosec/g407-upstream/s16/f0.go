package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesCBC := cipher.NewCBCEncrypter(block, []byte("ILoveMyNonceAlot"))

	var output = make([]byte, 16)
	aesCBC.CryptBlocks(output, []byte("Very Cool thing!"))
	fmt.Println(string(output))

	aesCBC = cipher.NewCBCDecrypter(block, []byte("ILoveMyNonceAlot"))
	aesCBC.CryptBlocks(output, output)
	fmt.Println(string(output))

}