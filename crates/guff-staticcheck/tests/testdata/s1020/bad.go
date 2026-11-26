package main

func f(i interface{}) {
	if _, ok := i.(int); ok && i != nil {}
}
