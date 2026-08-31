package log

func Fatal(v ...any)                 {}
func Fatalf(format string, v ...any) {}
func Fatalln(v ...any)               {}
func Panic(v ...any)                 {}
func Panicf(format string, v ...any) {}
func Panicln(v ...any)               {}

type Logger struct{}

// The real signature takes an `io.Writer`; `any` keeps the stub set small.
func New(out any, prefix string, flag int) *Logger { return &Logger{} }

func (l *Logger) Fatal(v ...any)                 {}
func (l *Logger) Fatalf(format string, v ...any) {}
func (l *Logger) Fatalln(v ...any)               {}
func (l *Logger) Panic(v ...any)                 {}
func (l *Logger) Panicf(format string, v ...any) {}
func (l *Logger) Panicln(v ...any)               {}
