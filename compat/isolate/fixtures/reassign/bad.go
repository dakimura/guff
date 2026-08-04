package p

import "io"

func Bad() {
	io.EOF = nil
}
