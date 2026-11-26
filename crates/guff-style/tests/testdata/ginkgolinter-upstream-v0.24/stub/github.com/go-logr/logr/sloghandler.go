package logr

import (
	"context"
	"log/slog"
)

type slogHandler struct {
	sink        LogSink
	slogSink    SlogSink
	groupPrefix string
	levelBias   slog.Level
}

var _ slog.Handler = &slogHandler{}

const groupSeparator = "."

func (l *slogHandler) GetLevel() slog.Level {
	panic("stub")
}
func (l *slogHandler) Enabled(_ context.Context, level slog.Level) bool {
	panic("stub")
}
func (l *slogHandler) Handle(ctx context.Context, record slog.Record) error {
	panic("stub")
}
func (l *slogHandler) sinkWithCallDepth() LogSink {
	panic("stub")
}
func (l *slogHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	panic("stub")
}
func (l *slogHandler) WithGroup(name string) slog.Handler {
	panic("stub")
}
func attrToKVs(attr slog.Attr, groupPrefix string, kvList []any) []any {
	panic("stub")
}
func addPrefix(prefix, name string) string {
	panic("stub")
}
func (l *slogHandler) levelFromSlog(level slog.Level) int {
	panic("stub")
}
