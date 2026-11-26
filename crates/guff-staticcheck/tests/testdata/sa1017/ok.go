package main

import (
	"os"
	"os/signal"
)

func main() {
	c1 := make(chan os.Signal, 1)
	signal.Notify(c1, os.Interrupt)
}
