package bufio

import "io"

type Reader struct{}

func NewReader(rd io.Reader) *Reader { return nil }

func (b *Reader) ReadString(delim byte) (string, error) { return "", nil }
