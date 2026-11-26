package main

func f(bs []byte, offset, n int) {
	for i := 0; i < n; i++ {
		bs[i] = bs[offset+i]
	}
}
