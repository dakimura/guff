package p

import (
	"context"
	"log/slog"
)

// Each function below trips one of sloglint's rules. They are separate
// messages with separate positions; a fixture with one mixed-args call reaches
// exactly one of them.

// no-mixed-args
func MixedArgs() {
	slog.Info("msg", "k", 1, slog.Int("n", 2))
}

// static-msg
func DynamicMessage(name string) {
	slog.Info("hello " + name)
}

// no-raw-keys
func RawKey() {
	slog.Info("msg", "raw", 1)
}

// key-naming-case: snake
func WrongKeyCase() {
	slog.Info("msg", slog.Int("notSnake", 1))
}

// forbidden-keys
func ForbiddenKey() {
	slog.Info("msg", slog.Int("banned", 1))
}

// context: scope — a context is in scope but the non-Context call was used.
func MissingContext(ctx context.Context) {
	_ = ctx
	slog.Info("msg")
}
