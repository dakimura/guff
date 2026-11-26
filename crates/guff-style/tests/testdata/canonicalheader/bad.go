package canonicalheader

import "net/http"

const constTestHeader = "testHeaderValue"

func bad() {
	v := http.Header{}
	v.Get(constTestHeader)
	v.Get("Test-HEader")
	v.Set("Test-HEader", "value")
	v.Add("Test-HEader", "value")
	v.Del("Test-HEader")
	v.Values("Test-HEader")
	v.Values(`Raw-STRING-Literal`)
	v.Get("etag")
}
