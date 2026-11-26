package http

type Request struct{}
type Header map[string][]string

func NewRequest(method, url string, body any) (*Request, error) {
	return nil, nil
}

func NewRequestWithContext(ctx any, method, url string, body any) (*Request, error) {
	return nil, nil
}
