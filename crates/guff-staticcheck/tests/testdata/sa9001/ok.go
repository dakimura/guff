package main
func f(ch chan int) { for range ch { break; defer func(){}() } }
