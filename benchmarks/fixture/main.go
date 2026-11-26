package main

import (
	"fmt"
	"os"
)

func mayFail() error {
	return fmt.Errorf("boom")
}

func unusedHelper() int {
	return 42
}

func main() {
	mayFail() // unchecked error (errcheck)
	x := 1
	x = 2 // ineffassign
	_ = x
	if len(os.Args) > 1 {
		fmt.Println(os.Args[1])
	}
}
