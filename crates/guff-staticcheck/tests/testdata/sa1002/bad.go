package main

import "time"

func main() {
	time.Parse("12345", "")
	time.Parse("not-a-layout", "")
}
