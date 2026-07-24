package slog

type Logger struct{}

type Attr struct{}

type Record struct{}

func Info(msg string, args ...any) {}
func Error(msg string, args ...any) {}
func Any(key string, value any) Attr  { return Attr{} }
func String(key, value string) Attr   { return Attr{} }
