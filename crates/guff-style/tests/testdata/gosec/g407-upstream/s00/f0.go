package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func main() {

	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesOFB := cipher.NewOFB(block, []byte("ILoveMyNonceAlot"))
	var output = make([]byte, 16)
	aesOFB.XORKeyStream(output, []byte("Very Cool thing!"))
	fmt.Println(string(output))

}
