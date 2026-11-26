package gbytes

import (
	"errors"
	"io"
	"time"
)

var ErrTimeout = errors.New("timeout occurred")

func TimeoutCloser(c io.Closer, timeout time.Duration) io.Closer {
	panic("stub")
}
func TimeoutReader(r io.Reader, timeout time.Duration) io.Reader {
	panic("stub")
}
func TimeoutWriter(w io.Writer, timeout time.Duration) io.Writer {
	panic("stub")
}

type timeoutReaderWriterCloser struct {
	c io.Closer
	w io.Writer
	r io.Reader
	d time.Duration
}

func (t timeoutReaderWriterCloser) Close() error {
	panic("stub")
}
func (t timeoutReaderWriterCloser) Read(p []byte) (int, error) {
	panic("stub")
}
func (t timeoutReaderWriterCloser) Write(p []byte) (int, error) {
	panic("stub")
}
