package main

func f(i interface{}) {
	switch x := i.(type) {
	case int:
		_ = x
	}
}
