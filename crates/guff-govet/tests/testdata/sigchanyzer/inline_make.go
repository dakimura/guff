package p

import (
	"os"
	"os/signal"
)

// Upstream deliberately exempts a channel created inline by make:
// "Only signal.Notify(make(chan os.Signal), os.Interrupt) is safe,
// conservatively treat others as not safe" (golang/go#45043).
func g() {
	signal.Notify(make(chan os.Signal), os.Interrupt)
}
