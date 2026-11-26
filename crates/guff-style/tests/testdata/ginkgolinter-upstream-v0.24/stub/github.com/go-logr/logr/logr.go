package logr

func New(sink LogSink) Logger {
	panic("stub")
}
func (l *Logger) setSink(sink LogSink) {
	panic("stub")
}
func (l Logger) GetSink() LogSink {
	panic("stub")
}
func (l Logger) WithSink(sink LogSink) Logger {
	panic("stub")
}

type Logger struct {
	sink  LogSink
	level int
}

func (l Logger) Enabled() bool {
	panic("stub")
}
func (l Logger) Info(msg string, keysAndValues ...any) {
	panic("stub")
}
func (l Logger) Error(err error, msg string, keysAndValues ...any) {
	panic("stub")
}
func (l Logger) V(level int) Logger {
	panic("stub")
}
func (l Logger) GetV() int {
	panic("stub")
}
func (l Logger) WithValues(keysAndValues ...any) Logger {
	panic("stub")
}
func (l Logger) WithName(name string) Logger {
	panic("stub")
}
func (l Logger) WithCallDepth(depth int) Logger {
	panic("stub")
}
func (l Logger) WithCallStackHelper() (func(), Logger) {
	panic("stub")
}
func (l Logger) IsZero() bool {
	panic("stub")
}

type RuntimeInfo struct{ CallDepth int }

var runtimeInfo = RuntimeInfo{CallDepth: 1}

type LogSink interface {
	Init(info RuntimeInfo)
	Enabled(level int) bool
	Info(level int, msg string, keysAndValues ...any)
	Error(err error, msg string, keysAndValues ...any)
	WithValues(keysAndValues ...any) LogSink
	WithName(name string) LogSink
}
type CallDepthLogSink interface{ WithCallDepth(depth int) LogSink }
type CallStackHelperLogSink interface{ GetCallStackHelper() func() }
type Marshaler interface{ MarshalLog() any }
