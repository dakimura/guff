package main

import "time"

func main() {
	time.Parse("2006-01-02", "2020-01-01")
	time.Parse("2006", "2006")
}
