package loggercheck

import "log/slog"

func requireKeyBad() {
	key := "dyn"
	slog.Info("msg", key, "value")
}

func printfLikeBad() {
	slog.Info("status %d", "key", "value")
}
