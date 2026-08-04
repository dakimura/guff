package p

import "fmt"

func Bad(s string, n int) string {
	_ = fmt.Sprintf("%s", s)
	return fmt.Sprintf("%d", n)
}
