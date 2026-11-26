package p

import (
	// A blank import of the same module is a second finding, and it reports at
	// the `_`: `ast.ImportSpec.Pos()` is the *name* when the spec has one,
	// which is two columns before the path literal guff used to report.
	_ "github.com/sirupsen/logrus"

	"github.com/sirupsen/logrus"
)

func Bad() {
	logrus.Info("x")
}

// A second *use* of the blocked module adds no finding: gomodguard reports the
// `import` statement, once, not each call through it.
func AlsoBad() {
	logrus.Warn("y")
}
