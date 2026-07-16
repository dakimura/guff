package sloglint

import "log/slog"

func badMixed() {
	slog.Info("msg", "foo", 1, slog.Int("bar", 2))
	slog.Warn("msg", "foo", 1, slog.String("bar", "x"))
	logger := slog.With("k", "v")
	logger.Error("msg", "foo", 1, slog.Int("bar", 2))
}
