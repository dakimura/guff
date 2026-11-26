package des
type Cipher interface{ BlockSize() int }
func NewCipher(key []byte) (Cipher, error) { return nil, nil }
func NewTripleDESCipher(key []byte) (Cipher, error) { return nil, nil }
