package p

import "errors"

func Bad() error {
	return errors.New("dynamic")
}
