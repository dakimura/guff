package main

import "fmt"

func fn(s string) {
	fmt.Printf("%s", s)
	fmt.Print(s)
}

func main() { fn("x") }
