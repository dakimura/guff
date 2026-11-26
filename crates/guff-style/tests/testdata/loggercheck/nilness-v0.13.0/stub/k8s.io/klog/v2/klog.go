// Package klog is a guff-written stub of the k8s.io/klog/v2 API that
// loggercheck v0.13.0's nilness testdata calls: the signatures only. The
// loggercheck rules match on the function, and its nilness pass decides
// which calls never return by package path and name, so neither needs the
// real bodies.
package klog

import (
	"os"
	"time"
)

// OsExit is the function klog's exiting calls end in.
var OsExit = os.Exit

type Level int32

type Verbose struct{ enabled bool }

func V(level Level) Verbose { return Verbose{} }

func (v Verbose) Enabled() bool                                                  { return v.enabled }
func (v Verbose) Info(args ...interface{})                                       {}
func (v Verbose) Infof(format string, args ...interface{})                       {}
func (v Verbose) InfoS(msg string, keysAndValues ...interface{})                 {}
func (v Verbose) InfoSDepth(depth int, msg string, keysAndValues ...interface{}) {}
func (v Verbose) ErrorS(err error, msg string, keysAndValues ...interface{})     {}

func Info(args ...interface{})                                       {}
func InfoS(msg string, keysAndValues ...interface{})                 {}
func InfoSDepth(depth int, msg string, keysAndValues ...interface{}) {}
func Error(args ...interface{})                                      {}
func ErrorS(err error, msg string, keysAndValues ...interface{})     {}
func Exit(args ...interface{})                                       { OsExit(1) }
func Exitf(format string, args ...interface{})                       { OsExit(1) }
func Fatal(args ...interface{})                                      { OsExit(255) }
func Fatalf(format string, args ...interface{})                      { OsExit(255) }
func FlushAndExit(flushTimeout time.Duration, exitCode int)          { OsExit(exitCode) }
