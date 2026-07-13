package main

import "time"

func fn() {
	t := time.NewTimer(time.Second)
	if t.Reset(time.Second) {
		println("x")
	}
}
