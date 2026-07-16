package pkg

import "fmt"

func fn() {
	type ByteSlice []byte
	var b1 []byte
	var b2 ByteSlice
	var s string

	fmt.Print(1, b1, 2, []byte(""), b2, s)
	fmt.Fprint(nil, 1, b1, 2, []byte(""), b2, s)
	fmt.Print()
	fmt.Fprint(nil)
	fmt.Println(s)
}
