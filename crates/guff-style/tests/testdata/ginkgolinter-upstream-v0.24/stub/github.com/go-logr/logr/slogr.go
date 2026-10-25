package logr

import (
	"context"
	"log/slog"
)

func FromSlogHandler(handler slog.Handler) Logger {
	panic("stub")
}
func ToSlogHandler(logger Logger) slog.Handler {
	panic("stub")
}

type SlogSink interface {
	LogSink
	Handle(ctx context.Context, record slog.Record) error
	WithAttrs(attrs []slog.Attr) SlogSink
	WithGroup(name string) SlogSink
}
