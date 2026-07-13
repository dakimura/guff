package main

import "encoding/hex"

func main() {
	sliceA := make([]byte, 8)
	sliceB := make([]byte, 8)
	hex.Encode(sliceA, sliceB)
	hex.Encode(sliceA, sliceA)
	hex.Encode(sliceA[1:], sliceA[2:])
	hex.Encode(sliceA[1:], sliceA[1:])
	sliceC := sliceA
	hex.Encode(sliceA, sliceC)
	if true {
		hex.Encode(sliceA, sliceC)
	}
	sliceD := sliceA[1:]
	sliceE := sliceA[1:]
	if true {
		hex.Encode(sliceD, sliceE)
	}
	var b bool
	if !b && true {
		hex.Encode(sliceD, sliceE)
	}
}

func fooSigmaA(a *[4]byte) {
	low := 2
	x := a[low:]

	if true {
		y := a[low:]
		hex.Encode(x, y)
	}
}

func fooSigmaB(a *[4]byte) {
	x := a[:]

	if true {
		y := a[:]
		hex.Encode(x, y)
	}
}
