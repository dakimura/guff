package main
func f(ch chan int) { for range ch { defer func(){}() } }
