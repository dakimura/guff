package p

import "log/slog"

func f() {
	slog.Info("hello", "key")
}
