package gochecknoinits

var x int

func init() {
	x = 1
}

func init() {
	x = 2
}

func regular() {}
