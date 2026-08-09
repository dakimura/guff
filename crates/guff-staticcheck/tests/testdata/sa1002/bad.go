package main

import "time"

func main() {
	// Both error shapes time.Parse can produce for a self-parsing layout:
	// a field that runs out of input, and a field that is out of range.
	time.Parse("12345", "")
	time.Parse("1234", "")
	time.Parse("123456", "")
}
