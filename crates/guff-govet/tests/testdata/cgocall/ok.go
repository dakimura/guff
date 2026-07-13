package p

import "C"

func f(x C.int) {
	C.abs(x)
}
