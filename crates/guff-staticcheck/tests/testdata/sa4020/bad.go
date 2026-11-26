package main

type T struct{}

func main() {
	var v any = T{}
	switch v.(type) {
	case any:
	case T:
	}
}
