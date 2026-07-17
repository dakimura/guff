package gosec_ok

import (
	"crypto/sha256"
	"hash"
)

func ok() hash.Hash {
	return sha256.New()
}
