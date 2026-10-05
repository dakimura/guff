package main

import (
"crypto/cipher"
)

func myBadCipher(n int, block cipher.Block) cipher.Stream {
    iv := make([]byte, n) 
    iv[0] = 0x01
    return cipher.NewCTR(block, iv)
}
	   