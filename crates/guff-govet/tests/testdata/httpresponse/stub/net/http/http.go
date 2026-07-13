package http

type ReadCloser interface {
	Read([]byte) (int, error)
	Close() error
}

type Response struct {
	Body ReadCloser
}

type Client struct{}

func (c *Client) Get(url string) (*Response, error) {
	return nil, nil
}

func Get(url string) (*Response, error) {
	return nil, nil
}
