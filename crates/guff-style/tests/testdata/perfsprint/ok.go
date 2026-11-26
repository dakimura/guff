package p

import "fmt"

func ok() {
	_ = fmt.Sprintf("%s %d", "hello", 42)
	_ = fmt.Sprintf("%#v", 42)
	_ = fmt.Sprintf("%.2f", 1.5)
	_ = fmt.Sprint("a", "b")
	_ = fmt.Errorf("this is %s", "complex")
}
