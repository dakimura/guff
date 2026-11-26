package p

import "fmt"

func errErrorCases() {
	var err error
	_ = fmt.Sprintf("%s", err)
	_ = fmt.Sprintf("%v", err)
	_ = fmt.Sprint(err)
}
