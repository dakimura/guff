package main
import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
)

func main() {
	iv := make([]byte, 16)
	rand.Read(iv)
	
	// Alias assignment
	alias := iv
	alias[0] = 0 // Hardcoded write via alias (unsafe)

	block, _ := aes.NewCipher([]byte("12345678123456781234567812345678"))
	_ = cipher.NewCTR(block, iv)
}
