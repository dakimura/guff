package slog

type Attr struct{}

type Logger struct{}

type Level int

const LevelInfo Level = 0

func String(key, value string) Attr { return Attr{} }

func Int(key string, value int) Attr { return Attr{} }

func Group(key string, args ...any) Attr { return Attr{} }

func GroupAttrs(key string, attrs ...Attr) Attr { return Attr{} }

func With(args ...any) *Logger { return &Logger{} }

func Debug(msg string, args ...any) {}

func Info(msg string, args ...any) {}

func Warn(msg string, args ...any) {}

func Error(msg string, args ...any) {}

func DebugContext(ctx any, msg string, args ...any) {}

func InfoContext(ctx any, msg string, args ...any) {}

func WarnContext(ctx any, msg string, args ...any) {}

func ErrorContext(ctx any, msg string, args ...any) {}

func Log(ctx any, level Level, msg string, args ...any) {}

func LogAttrs(ctx any, level Level, msg string, attrs ...Attr) {}

func (l *Logger) With(args ...any) *Logger { return l }

func (l *Logger) Debug(msg string, args ...any) {}

func (l *Logger) Info(msg string, args ...any) {}

func (l *Logger) Warn(msg string, args ...any) {}

func (l *Logger) Error(msg string, args ...any) {}

func (l *Logger) DebugContext(ctx any, msg string, args ...any) {}

func (l *Logger) InfoContext(ctx any, msg string, args ...any) {}

func (l *Logger) WarnContext(ctx any, msg string, args ...any) {}

func (l *Logger) ErrorContext(ctx any, msg string, args ...any) {}

func (l *Logger) Log(ctx any, level Level, msg string, args ...any) {}

func (l *Logger) LogAttrs(ctx any, level Level, msg string, attrs ...Attr) {}
