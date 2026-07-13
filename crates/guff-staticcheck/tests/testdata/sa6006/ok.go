package main
import "io"
func f(w io.Writer, b []byte) { w.Write(b) }
