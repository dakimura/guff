package gbytes

import (
	"io"
	"regexp"
	"sync"
)

type Buffer struct {
	contents     []byte
	readCursor   uint64
	lock         *sync.Mutex
	detectCloser chan any
	closed       bool
}

func NewBuffer() *Buffer {
	panic("stub")
}
func BufferWithBytes(bytes []byte) *Buffer {
	panic("stub")
}
func BufferReader(reader io.Reader) *Buffer {
	panic("stub")
}
func (b *Buffer) Write(p []byte) (n int, err error) {
	panic("stub")
}
func (b *Buffer) Read(d []byte) (int, error) {
	panic("stub")
}
func (b *Buffer) Clear() error {
	panic("stub")
}
func (b *Buffer) Close() error {
	panic("stub")
}
func (b *Buffer) Closed() bool {
	panic("stub")
}
func (b *Buffer) Contents() []byte {
	panic("stub")
}
func (b *Buffer) Detect(desired string, args ...any) chan bool {
	panic("stub")
}
func (b *Buffer) CancelDetects() {
	panic("stub")
}
func (b *Buffer) didSay(re *regexp.Regexp) (bool, []byte) {
	panic("stub")
}
