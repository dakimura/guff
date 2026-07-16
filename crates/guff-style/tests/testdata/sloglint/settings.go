package sloglint

import (
	"context"
	"fmt"
	"log/slog"
)

var globalLogger *slog.Logger

const goodKey = "user_id"

func settingsCases(ctx context.Context) {
	slog.Info("msg") // no-global default
	globalLogger.Info("msg")

	slog.Info(fmt.Sprintf("dynamic"))
	slog.Info("Capitalized message")

	slog.Info("msg", "time", 1)
	slog.Info("msg", "user_id", 1)
	slog.Info("msg", goodKey, 1)

	slog.Info("msg", "foo", 1, "bar", 2)

	slog.Info("msg", "a", 1) // context all/scope

	_ = ctx
}
