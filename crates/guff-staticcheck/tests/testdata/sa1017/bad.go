package main

import (
	"os"
	"os/signal"
)

func main() {
	c0 := make(chan os.Signal)
	signal.Notify(c0, os.Interrupt)

	c1 := make(chan os.Signal, 1)
	signal.Notify(c1, os.Interrupt)
}
