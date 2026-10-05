package main

import (
	"crypto/rand"
	"crypto/cipher"
)

func pointerUnOpSafeIV(block cipher.Block) {
	iv := make([]byte, 16)
	rand.Read(iv) // Dynamic
	ptr := &iv
	stream := cipher.NewCTR(block, *ptr) // dynamic dereference
	_ = stream
}
