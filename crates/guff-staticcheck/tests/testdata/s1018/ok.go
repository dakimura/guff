package main

func f(bs []byte, offset, n int) { copy(bs[:n], bs[offset:]) }
