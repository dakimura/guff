package use

import (
	"example.com/printf/operands/xpkg/wrap"
)

func calls(l *wrap.Logger, b wrap.Box[int]) error {
	wrap.Infof("%d items", "x")
	wrap.Prefixed("%d items", "x")
	wrap.Info("count %d", 3)
	l.Debugf("%s", 1)
	b.Logf("%q", 1.5)
	wrap.NotAWrapper("%d", "x")
	return wrap.Errorf("%w", "not an error")
}
