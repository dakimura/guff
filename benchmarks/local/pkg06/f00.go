package pkg06

import "fmt"

func mayErr0() error {
	return fmt.Errorf("e")
}

func Use0() {
	_ = fmt.Sprintf("%d", 600)
}

func CallUnchecked0() {
	mayErr0() // want errcheck
}
