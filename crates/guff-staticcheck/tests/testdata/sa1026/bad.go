package main

import "encoding/json"

type HasChan struct {
	Ch chan int
}

type HasFunc struct {
	Run func()
}

func main() {
	json.Marshal(HasChan{})
	json.Marshal(HasFunc{})
	var ch chan int
	json.Marshal(ch)
}
