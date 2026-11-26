package main

import (

"crypto/aes"
"crypto/cipher"
"crypto/rand"

)

func myReaderDirect(b []byte) (int, error) {
	n, err := rand.Read(b)
	if n > 1 {
		b[0] = 1 // overwriting
	}
	return n, err
}

func main() {
	iv := make([]byte, 16)
	// Direct call to user function (myReaderDirect) which calls rand.Read but overwrites the IV
	myReaderDirect(iv)

	key := []byte("example key 1234")
	block, _ := aes.NewCipher(key)
	_ = cipher.NewCTR(block, iv)
}
	   