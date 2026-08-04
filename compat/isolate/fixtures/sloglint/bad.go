package p

import "log/slog"

func Bad() {
	slog.Info("msg", "k") // odd attrs
}
