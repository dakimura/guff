package pkg

import "fmt"

type StringerBytes []byte

func (StringerBytes) String() string { return "x" }

func fn() {
	var s string
	fmt.Print(s)
	fmt.Println(1, "ok")
	fmt.Fprint(nil, s)

	var sb StringerBytes
	fmt.Print(sb)
	fmt.Fprint(nil, sb)
}
