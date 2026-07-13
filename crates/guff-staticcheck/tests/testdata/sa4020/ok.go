package main

func main() {
	var v any = 1
	switch v.(type) {
	case int:
	case string:
	}
}
