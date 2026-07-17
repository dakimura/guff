package pkg

import "errors"

func fn() {
	errors.New("a perfectly fine error")
	errors.New("Not a great error")
	errors.New("also not a great error.")
	errors.New("URL is okay")
	errors.New("SomeFunc is okay")
	errors.New("T must not be nil")
	errors.New("Foo() failed")
	errors.New("P384 is a nice curve")
}

type T struct{}

func Write() {
	errors.New("Write: this is broken")
}

func (T) Read() {
	errors.New("Read failed")
}
