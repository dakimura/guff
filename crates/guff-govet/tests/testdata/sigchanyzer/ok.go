package p

import (
	"os"
	"os/signal"
)

func f() {
	c := make(chan os.Signal, 1)
	signal.Notify(c, os.Interrupt)
}
