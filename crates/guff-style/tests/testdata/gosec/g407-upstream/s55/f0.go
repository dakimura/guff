package main
import (
    "crypto/aes"
    "crypto/cipher"
    "os"
)
func main() {
    iv := make([]byte, 16)
    i := len(os.Args)
    iv[i] = 0
    block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
    _ = cipher.NewCTR(block, iv)
}
