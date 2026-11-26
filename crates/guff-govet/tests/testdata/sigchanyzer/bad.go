package p

import (
	"os"
	"os/signal"
)

func f() {
	c := make(chan os.Signal)
	signal.Notify(c, os.Interrupt)
}
