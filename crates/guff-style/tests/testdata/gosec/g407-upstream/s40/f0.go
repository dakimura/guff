package main

import (
"crypto/cipher"
)

func myBadCipher(n int, block cipher.Block) cipher.Stream {
    iv := make([]byte, n) 
    return cipher.NewCTR(block, iv)
}
	   