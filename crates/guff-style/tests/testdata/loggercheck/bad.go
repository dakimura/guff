package loggercheck

import "log/slog"

func badPackageLevel() {
	slog.Info("msg", "key1")
	slog.Info("msg", "key1", "value1", "key2")
}

func badLoggerMethod() {
	logger := slog.With("k", "v")
	logger.Info("msg", "key1")
	slog.With("key1").Info("msg")
}

func badWithAttr() {
	slog.Info("msg", slog.String("method", "POST"), "key_only")
	slog.Info("group", slog.Group("g", "gkey1", "gvalue1", "gkey2"))
}
