package main

import (
	"crypto/cipher"
	"io"
)

func interfaceSafeOverwrite(r io.Reader, block cipher.Block) {
	iv := make([]byte, 16)
	iv[0] = 0 // Tainted
	r.Read(iv) // Dynamic Interface Read (covers taint)
	stream := cipher.NewCTR(block, iv)
	_ = stream
}
