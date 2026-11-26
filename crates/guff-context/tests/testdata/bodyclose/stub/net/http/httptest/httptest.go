package httptest

import "net/http"

type ResponseRecorder struct{}

func (r *ResponseRecorder) Result() *http.Response {
	return nil
}
