package p

import (
	"os"
	"os/signal"
)

func f() {
	signal.Notify(make(chan os.Signal), os.Interrupt)
}
