package main

import (
"crypto/cipher"
"os"
)

func myGoodCipher(block cipher.Block) (cipher.Stream, error) {
    iv, err := os.ReadFile("iv.bin")
    if err != nil {
        return nil, err
    }
    return cipher.NewCTR(block, iv), nil
}
