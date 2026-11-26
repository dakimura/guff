package pkg

import "net/http"

func fn() {
	http.Error(nil, "", 506)
	http.Redirect(nil, nil, "", 506)
	http.StatusText(506)
	http.RedirectHandler("", 506)

	http.StatusText(600)
	http.StatusText(http.StatusAccepted)
	http.StatusText(404)
}
