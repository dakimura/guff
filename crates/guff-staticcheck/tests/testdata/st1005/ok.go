package pkg

import "errors"

func fn() {
	errors.New("ok lowercase")
	errors.New("URL is fine")
	errors.New("Write failed")
}

func Write() {}
