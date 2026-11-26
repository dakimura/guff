package main

func f(i interface{}) {
	switch i.(type) {
	case int:
		_ = i.(int)
	}
}
