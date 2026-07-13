package main

func main() {
	var p *int
	var i any
	i = p
	_ = i == nil
}
