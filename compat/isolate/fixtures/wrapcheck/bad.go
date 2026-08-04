package p

import "os"

func Bad() error {
	_, err := os.Open("missing")
	return err
}
