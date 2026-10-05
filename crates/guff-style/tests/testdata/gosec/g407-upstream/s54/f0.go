package main
import (
    "crypto/aes"
    "crypto/cipher"
    "crypto/rand"
    "os"
)
func main() {
    iv := make([]byte, 16)
    low := len(os.Args)
    sub := iv[low:]
    rand.Read(sub)
    block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
    _ = cipher.NewCTR(block, iv)
}
