package main

import (
	"crypto/cipher"
)

func pointerUnOpIV(block cipher.Block) {
	iv := make([]byte, 16) // Hardcoded
	ptr := &iv
	stream := cipher.NewCTR(block, *ptr)
	_ = stream
}
