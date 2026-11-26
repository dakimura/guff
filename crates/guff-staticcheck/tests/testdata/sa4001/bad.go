package main

type T struct{}

func main() {
	t1 := &T{}
	_ = t1
	_ = &*t1
	_ = *&t1
}
