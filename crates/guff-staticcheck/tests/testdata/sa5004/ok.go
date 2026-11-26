package main

func f() {
	var ch chan int
	for {
		select {
		case i := <-ch:
			_ = i
		default:
			return
		}
	}
}
