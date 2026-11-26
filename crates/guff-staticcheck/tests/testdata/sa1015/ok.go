package main

import "time"

func main() {
	for range time.Tick(0) {
		println("")
	}
}
