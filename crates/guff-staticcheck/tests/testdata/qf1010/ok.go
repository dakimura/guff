package pkg

import "fmt"

func fn() {
	var s string
	fmt.Print(s)
	fmt.Println(1, "ok")
	fmt.Fprint(nil, s)
}
