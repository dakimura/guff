package gexec

import (
	"io"
	"sync"
)

type PrefixedWriter struct {
	prefix        []byte
	writer        io.Writer
	lock          *sync.Mutex
	atStartOfLine bool
}

func NewPrefixedWriter(prefix string, writer io.Writer) *PrefixedWriter {
	panic("stub")
}
func (w *PrefixedWriter) Write(b []byte) (int, error) {
	panic("stub")
}
