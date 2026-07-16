package pkg

import "net/http"

func fn() {
	http.Error(nil, "", http.StatusOK)
	http.StatusText(200)
	http.StatusText(404)
	http.StatusText(500)
	http.StatusText(http.StatusAccepted)
}
