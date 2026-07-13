package main

func f(ch chan int) {
	v := <-ch
	_ = v
}
