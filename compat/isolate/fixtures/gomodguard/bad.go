package p

import "github.com/sirupsen/logrus"

func Bad() {
	logrus.Info("x")
}

// A second *use* of the blocked module adds no finding: gomodguard reports the
// `import` statement, once, not each call through it.
func AlsoBad() {
	logrus.Warn("y")
}
