package sloglint

import "log/slog"

func okKv() {
	slog.Info("msg", "foo", 1, "bar", 2)
	slog.Info("msg", "foo", 1, slog.Group("g", "bar", 2))
}

func okAttrs() {
	slog.Info("msg", slog.Int("foo", 1), slog.Int("bar", 2))
}

func okLogger() {
	logger := slog.With("k", "v")
	logger.Info("msg", "foo", 1)
}
