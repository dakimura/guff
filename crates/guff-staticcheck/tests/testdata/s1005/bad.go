package main

func f(ch chan int) {
	_ = <-ch
}
