package io
type ReadCloser interface { Read([]byte) (int, error); Close() error }
