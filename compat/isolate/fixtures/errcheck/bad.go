package p

import (
	"fmt"
	"os"
)

func mayFail() error {
	return fmt.Errorf("boom")
}

func Bad() {
	mayFail()
	os.Remove("nope") // unchecked
	_ = mayFail()     // blank assign still unchecked depending on settings
}
