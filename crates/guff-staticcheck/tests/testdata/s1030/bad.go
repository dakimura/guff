package main

import "bytes"

func f(buf bytes.Buffer) string { return string(buf.Bytes()) }
