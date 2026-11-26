package main

import "bytes"

func f(buf bytes.Buffer) string { return string(buf.Bytes()) }

func g(buf *bytes.Buffer) []byte { return []byte(buf.String()) }

func h(buf bytes.Buffer) []byte { return []byte(buf.String()) }

func sel(w struct{ B bytes.Buffer }) string { return string(w.B.Bytes()) }
