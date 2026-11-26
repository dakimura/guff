package slog

type Attr struct{}

type Logger struct{}

func String(key, value string) Attr { return Attr{} }

func Int(key string, value int) Attr { return Attr{} }

func Group(key string, args ...any) Attr { return Attr{} }

func With(args ...any) *Logger { return &Logger{} }

func Debug(msg string, args ...any) {}

func Info(msg string, args ...any) {}

func Warn(msg string, args ...any) {}

func Error(msg string, args ...any) {}

func (l *Logger) With(args ...any) *Logger { return l }

func (l *Logger) Debug(msg string, args ...any) {}

func (l *Logger) Info(msg string, args ...any) {}

func (l *Logger) Warn(msg string, args ...any) {}

func (l *Logger) Error(msg string, args ...any) {}
