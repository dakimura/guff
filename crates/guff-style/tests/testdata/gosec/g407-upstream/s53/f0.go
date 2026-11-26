package main
import (
    "crypto/aes"
    "crypto/cipher"
    "os"
)
func main() {
    iv := make([]byte, len(os.Args))
    block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
    _ = cipher.NewCTR(block, iv)
}
