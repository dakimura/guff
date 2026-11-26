package defaultexclude

import (
	"fmt"
	"hash"
)

func printIt() {
	fmt.Println("hello")
}

func hashWrite(h hash.Hash) {
	h.Write([]byte("x"))
}
