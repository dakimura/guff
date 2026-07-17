package canonicalheader

import "net/http"

func ok() {
	v := http.Header{}
	v.Set("Test-Header", "value")
	v.Add("Test-Header", "value")
	v.Del("Test-Header")
	v.Values("Test-Header")
	v.Get("ETag")
	v.Get("WWW-Authenticate")

	var someString = ""
	v.Get(someString)
}
