package main

import "encoding/hex"

func main() {
	sliceA := make([]byte, 8)
	sliceB := make([]byte, 8)
	hex.Encode(sliceA, sliceB)
	hex.Encode(sliceA[1:], sliceA[2:])
}
