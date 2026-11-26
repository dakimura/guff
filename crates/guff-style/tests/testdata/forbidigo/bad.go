package forbidigo_bad

import "fmt"

func bad() {
	fmt.Println("debug")
	fmt.Printf("x=%d", 1)
	print("builtin")
	println("builtin")
}
