package logr

import (
	"context"
	"log/slog"
)

func FromContext(ctx context.Context) (Logger, error) {
	panic("stub")
}
func FromContextAsSlogLogger(ctx context.Context) *slog.Logger {
	panic("stub")
}
func FromContextOrDiscard(ctx context.Context) Logger {
	panic("stub")
}
func NewContext(ctx context.Context, logger Logger) context.Context {
	panic("stub")
}
func NewContextWithSlogLogger(ctx context.Context, logger *slog.Logger) context.Context {
	panic("stub")
}
