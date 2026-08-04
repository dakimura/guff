package p

import "net/http"

func Bad() string {
	return "GET"
}

func BadStatus() int {
	_ = http.StatusOK
	return 200
}
