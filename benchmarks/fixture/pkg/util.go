package pkg

import "fmt"

// Unused is deliberately unused at package level for the unused linter.
func Unused() string {
	return "unused"
}

func Check(err error) {
	if err != nil {
		fmt.Println(err)
	}
}

func CallUnchecked() {
	mayErr()
}

func mayErr() error {
	return fmt.Errorf("oops")
}
