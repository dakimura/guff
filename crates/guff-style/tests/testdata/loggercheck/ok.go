package loggercheck

import "log/slog"

func okPackageLevel() {
	slog.Info("msg", "key1", "value1")
	slog.Info("msg", "key1", "value1", "key2", "value2")
}

func okLoggerMethod() {
	logger := slog.With("k", "v")
	logger.Info("msg", "key1", "value1")
	slog.With("key1", "value1").Info("msg")
}

func okWithAttr() {
	slog.Info("msg", slog.String("method", "POST"), slog.Int("status", 301))
	slog.Info("group", slog.Group("g", "gkey1", "gvalue1", "gkey2", "gvalue2"))
	slog.Info("mixed", "key1", "value1", slog.String("method", "POST"))
}
