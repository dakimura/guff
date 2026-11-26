package main
import "io"
func f(w io.Writer, b []byte) { io.WriteString(w, string(b)) }
