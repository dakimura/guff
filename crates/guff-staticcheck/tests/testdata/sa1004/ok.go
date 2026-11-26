package main

import "time"

const c1 = 1
const c2 = 200

func main() {
	time.Sleep(201)
	time.Sleep(c1)
	time.Sleep(c2)
	time.Sleep(0)
	time.Sleep(2 * time.Nanosecond)
	time.Sleep(time.Nanosecond)
}
