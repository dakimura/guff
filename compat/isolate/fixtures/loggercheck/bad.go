package p

import "log/slog"

func Bad() {
	slog.Info("msg", "only_key") // odd keyvals
}
