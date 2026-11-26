//nolint:errcheck // file-level, and it really does suppress mkerr() below
package main

func mkerr() error { return nil }

func used() {
	mkerr()
}

func unused() int {
	x := 1
	x = 2 //nolint
	return x
}

func main() {}
