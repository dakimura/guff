package logr

import (
	"log/slog"
)

var (
	_ LogSink          = &slogSink{}
	_ CallDepthLogSink = &slogSink{}
	_ Underlier        = &slogSink{}
)

type Underlier interface{ GetUnderlying() slog.Handler }

const (
	nameKey = "logger"
	errKey  = "err"
)

type slogSink struct {
	callDepth int
	name      string
	handler   slog.Handler
}

func (l *slogSink) Init(info RuntimeInfo) {
	panic("stub")
}
func (l *slogSink) GetUnderlying() slog.Handler {
	panic("stub")
}
func (l *slogSink) WithCallDepth(depth int) LogSink {
	panic("stub")
}
func (l *slogSink) Enabled(level int) bool {
	panic("stub")
}
func (l *slogSink) Info(level int, msg string, kvList ...interface{}) {
	panic("stub")
}
func (l *slogSink) Error(err error, msg string, kvList ...interface{}) {
	panic("stub")
}
func (l *slogSink) log(err error, msg string, level slog.Level, kvList ...interface{}) {
	panic("stub")
}
func (l slogSink) WithName(name string) LogSink {
	panic("stub")
}
func (l slogSink) WithValues(kvList ...interface{}) LogSink {
	panic("stub")
}
func kvListToAttrs(kvList ...interface{}) []slog.Attr {
	panic("stub")
}
