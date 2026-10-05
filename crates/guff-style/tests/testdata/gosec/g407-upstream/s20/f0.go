package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"fmt"
)

func coolFunc(size int) []byte{
	buf := make([]byte, size)
	rand.Read(buf)
	return buf
}

func main() {

	var nonce = coolFunc(16)
	block, _ := aes.NewCipher([]byte{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1})
	aesCTR := cipher.NewCTR(block, nonce)
	var output = make([]byte, 16)
	aesCTR.XORKeyStream(output, []byte("Very Cool thing!"))
	fmt.Println(string(output))
}
