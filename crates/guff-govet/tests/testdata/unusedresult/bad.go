package bad

import "errors"

func bad() {
	errors.New("x")
}
