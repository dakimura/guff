package p

import "C"

func f(ch chan int) {
	C.fn(ch)
}
