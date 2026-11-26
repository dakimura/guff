package assert

func main() {
	var i interface{}
	_ = i.(string)

	handleInterface(i.(string))

	if i.(string) == "hello" {
	}

	switch i.(type) {
	case string:
	case int:
		_ = i.(int)
	case nil:
	}
}

func handleInterface(i interface{}) string {
	return i.(string)
}
