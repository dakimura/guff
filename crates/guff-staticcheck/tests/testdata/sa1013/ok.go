package main

import "io"

func main() {
	var s io.Seeker
	s.Seek(0, io.SeekStart)
}
