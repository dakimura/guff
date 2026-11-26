package localerrors

import "example.com/local/errors"

func fn() {
	// Capitalized string via a non-stdlib errors.New must not trip ST1005.
	errors.New("Device IDs are not supported on Windows")
	errors.New("Invalid key for repository")
}
