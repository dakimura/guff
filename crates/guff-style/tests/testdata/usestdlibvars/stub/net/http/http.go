package http

const (
	MethodGet    = "GET"
	MethodPost   = "POST"
	StatusOK     = 200
	StatusNotFound = 404
)

func NewRequest(method, url string, body any) (*Request, error) {
	return nil, nil
}

func StatusText(code int) string { return "" }

type Request struct {
	Method string
}

type Response struct {
	StatusCode int
}
