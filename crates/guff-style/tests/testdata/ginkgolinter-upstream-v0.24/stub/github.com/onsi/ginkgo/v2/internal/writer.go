package internal

import (
	"bytes"
	"io"
	"sync"
	"github.com/go-logr/logr"
)

type WriterMode uint

const (
	WriterModeStreamAndBuffer WriterMode = iota
	WriterModeBufferOnly
)

type WriterInterface interface {
	io.Writer
	Truncate()
	Bytes() []byte
	Len() int
}
type Writer struct {
	buffer       *bytes.Buffer
	outWriter    io.Writer
	lock         *sync.Mutex
	mode         WriterMode
	streamIndent []byte
	indentNext   bool
	teeWriters   []io.Writer
}

func NewWriter(outWriter io.Writer) *Writer {
	panic("stub")
}
func (w *Writer) SetMode(mode WriterMode) {
	panic("stub")
}
func (w *Writer) Len() int {
	panic("stub")
}

var newline = []byte("\n")

func (w *Writer) Write(b []byte) (n int, err error) {
	panic("stub")
}
func (w *Writer) Truncate() {
	panic("stub")
}
func (w *Writer) Bytes() []byte {
	panic("stub")
}
func (w *Writer) TeeTo(writer io.Writer) {
	panic("stub")
}
func (w *Writer) ClearTeeWriters() {
	panic("stub")
}
func (w *Writer) Print(a ...any) {
	panic("stub")
}
func (w *Writer) Printf(format string, a ...any) {
	panic("stub")
}
func (w *Writer) Println(a ...any) {
	panic("stub")
}
func GinkgoLogrFunc(writer *Writer) logr.Logger {
	panic("stub")
}
