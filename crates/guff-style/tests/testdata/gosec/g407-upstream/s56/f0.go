package main
import (
    "crypto/aes"
    "crypto/cipher"
)
func test(iv []byte) {
    iv[0] = 0
    block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
    _ = cipher.NewCTR(block, iv)
}
func main() {
    test(make([]byte, 16))
}
