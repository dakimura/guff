package p

import "net/http"

func Bad() error {
	_, err := http.Get("http://example.com")
	return err
}
