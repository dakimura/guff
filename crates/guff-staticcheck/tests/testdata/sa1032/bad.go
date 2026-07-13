package main

import (
	"errors"
	"io"
)

func main() {
	var err error
	errors.Is(io.EOF, err)
}
