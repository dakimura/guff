package main

import (
"crypto/cipher"
"io"
)

func myGoodInterfaceCipher(r io.Reader, block cipher.Block) {
    iv := make([]byte, 16)
    r.Read(iv)
    stream := cipher.NewCTR(block, iv)
    _ = stream
}
