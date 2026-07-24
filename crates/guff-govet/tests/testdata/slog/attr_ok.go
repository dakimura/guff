package p

import "log/slog"

func g(err error) {
	slog.Error("failed", slog.Any("error", err))
	slog.Error("parse", slog.String("version", "1.0"), slog.Any("error", err))
	slog.Info("ok", slog.String("k", "v"))
}
