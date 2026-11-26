package main

import (

"crypto/aes"
"crypto/cipher"
"crypto/rand"

)

func myReaderDirect(b []byte) (int, error) {
	return rand.Read(b)
}

func main() {
	iv := make([]byte, 16)
	// Direct call to user function (myReaderDirect) which calls rand.Read
	myReaderDirect(iv)

	key := []byte("example key 1234")
	block, _ := aes.NewCipher(key)
	_ = cipher.NewCTR(block, iv)
}
	   