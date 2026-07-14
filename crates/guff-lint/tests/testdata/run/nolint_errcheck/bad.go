package main

func returnsError() error {
	return nil
}

func main() {
	returnsError() //nolint:errcheck
}
