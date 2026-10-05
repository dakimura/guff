package main

import (
	"crypto/cipher"
	"io"
)

func interfaceSafeOverwriteSlice(r io.Reader, block cipher.Block) {
	iv := make([]byte, 16)
	iv[0] = 0
	r.Read(iv[:])
	stream := cipher.NewCTR(block, iv)
	_ = stream
}
