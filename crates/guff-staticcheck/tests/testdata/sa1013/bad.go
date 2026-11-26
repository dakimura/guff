package main

import "io"

func main() {
	var s io.Seeker
	s.Seek(io.SeekStart, 0)
}
