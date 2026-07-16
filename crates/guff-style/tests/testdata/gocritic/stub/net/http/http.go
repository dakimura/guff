package http

import "io"

type Request struct{}

type ResponseWriter interface{}

var NoBody io.ReadCloser

func NewRequest(method, url string, body io.Reader) (*Request, error) {
	return &Request{}, nil
}

func NewRequestWithContext(ctx interface{}, method, url string, body io.Reader) (*Request, error) {
	return &Request{}, nil
}
