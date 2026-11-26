package bufio

import "io"

type Reader struct{}

func NewReader(rd io.Reader) *Reader        { return nil }
func (b *Reader) ReadString(delim byte) (string, error) { return "", nil }

type Scanner struct{}

func NewScanner(r io.Reader) *Scanner { return nil }
func (s *Scanner) Scan() bool         { return false }
func (s *Scanner) Text() string       { return "" }
