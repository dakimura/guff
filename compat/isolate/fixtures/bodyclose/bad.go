package p

import "net/http"

func Bad() error {
	resp, err := http.Get("http://example.com")
	if err != nil {
		return err
	}
	_ = resp
	return nil
}
