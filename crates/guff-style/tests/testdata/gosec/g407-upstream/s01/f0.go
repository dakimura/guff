package main

import (
	"crypto/aes"
	"crypto/cipher"
	"fmt"
)

func encrypt(nonce []byte) {
	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesOFB := cipher.NewOFB(block, nonce)
	var output = make([]byte, 16)
	aesOFB.XORKeyStream(output, []byte("Very Cool thing!"))
	fmt.Println(string(output))
}

func main() {

	var nonce = []byte("ILoveMyNonceAlot")
	encrypt(nonce)
}
