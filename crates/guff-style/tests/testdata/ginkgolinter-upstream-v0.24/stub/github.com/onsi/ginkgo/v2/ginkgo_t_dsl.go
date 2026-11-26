package ginkgo

import (
	"context"
	"io"
	"testing"
)

func GinkgoT(optionalOffset ...int) FullGinkgoTInterface {
	panic("stub")
}

type GinkgoTInterface interface {
	Cleanup(func())
	Chdir(dir string)
	Context() context.Context
	Setenv(kev, value string)
	Error(args ...any)
	Errorf(format string, args ...any)
	Fail()
	FailNow()
	Failed() bool
	Fatal(args ...any)
	Fatalf(format string, args ...any)
	Helper()
	Log(args ...any)
	Logf(format string, args ...any)
	Name() string
	Parallel()
	Skip(args ...any)
	SkipNow()
	Skipf(format string, args ...any)
	Skipped() bool
	TempDir() string
	Attr(key, value string)
	Output() io.Writer
	ArtifactDir() string
}
type FullGinkgoTInterface interface {
	GinkgoTInterface
	AddReportEntryVisibilityAlways(name string, args ...any)
	AddReportEntryVisibilityFailureOrVerbose(name string, args ...any)
	AddReportEntryVisibilityNever(name string, args ...any)
	Print(a ...any)
	Printf(format string, a ...any)
	Println(a ...any)
	F(format string, args ...any) string
	Fi(indentation uint, format string, args ...any) string
	Fiw(indentation uint, maxWidth uint, format string, args ...any) string
	RenderTimeline() string
	GinkgoRecover()
	DeferCleanup(args ...any)
	RandomSeed() int64
	ParallelProcess() int
	ParallelTotal() int
	AttachProgressReporter(func() string) func()
}

func GinkgoTB(optionalOffset ...int) *GinkgoTBWrapper {
	panic("stub")
}

type GinkgoTBWrapper struct {
	testing.TB
	GinkgoT FullGinkgoTInterface
}

func (g *GinkgoTBWrapper) Cleanup(f func()) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Chdir(dir string) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Context() context.Context {
	panic("stub")
}
func (g *GinkgoTBWrapper) Error(args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Errorf(format string, args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Fail() {
	panic("stub")
}
func (g *GinkgoTBWrapper) FailNow() {
	panic("stub")
}
func (g *GinkgoTBWrapper) Failed() bool {
	panic("stub")
}
func (g *GinkgoTBWrapper) Fatal(args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Fatalf(format string, args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Helper() {
	panic("stub")
}
func (g *GinkgoTBWrapper) Log(args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Logf(format string, args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Name() string {
	panic("stub")
}
func (g *GinkgoTBWrapper) Setenv(key, value string) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Skip(args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) SkipNow() {
	panic("stub")
}
func (g *GinkgoTBWrapper) Skipf(format string, args ...any) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Skipped() bool {
	panic("stub")
}
func (g *GinkgoTBWrapper) TempDir() string {
	panic("stub")
}
func (g *GinkgoTBWrapper) Attr(key, value string) {
	panic("stub")
}
func (g *GinkgoTBWrapper) Output() io.Writer {
	panic("stub")
}
func (g *GinkgoTBWrapper) ArtifactDir() string {
	panic("stub")
}
