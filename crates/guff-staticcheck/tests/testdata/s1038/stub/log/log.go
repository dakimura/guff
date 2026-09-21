// Package log stands in for the real one: what matters here is that the
// methods of `*log.Logger` and the package functions of the same names are
// *different objects*, so `typeutil.FuncName` tells them apart even though
// package path plus object name does not.
package log

type Logger struct{}

func (l *Logger) Print(v ...interface{})                 {}
func (l *Logger) Println(v ...interface{})               {}
func (l *Logger) Printf(format string, v ...interface{}) {}
func (l *Logger) Fatal(v ...interface{})                 {}
func (l *Logger) Fatalln(v ...interface{})               {}
func (l *Logger) Fatalf(format string, v ...interface{}) {}
func (l *Logger) Panic(v ...interface{})                 {}
func (l *Logger) Panicln(v ...interface{})               {}
func (l *Logger) Panicf(format string, v ...interface{}) {}

func Print(v ...interface{})                 {}
func Println(v ...interface{})               {}
func Printf(format string, v ...interface{}) {}
func Fatal(v ...interface{})                 {}
func Fatalln(v ...interface{})               {}
func Fatalf(format string, v ...interface{}) {}
func Panic(v ...interface{})                 {}
func Panicln(v ...interface{})               {}
func Panicf(format string, v ...interface{}) {}
