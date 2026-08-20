package http

type Request struct{}

type Header map[string][]string

type Body interface {
	Read(p []byte) (int, error)
	Close() error
}

type Response struct {
	Status     string
	StatusCode int
	Header     Header
	Body       Body
}

type Client struct{}

func Get(url string) (*Response, error) {
	return nil, nil
}

func Head(url string) (*Response, error) {
	return nil, nil
}

func Post(url, contentType string, body any) (*Response, error) {
	return nil, nil
}

func (c *Client) Do(req *Request) (*Response, error) {
	return nil, nil
}

func NewRequest(method, url string, body any) (*Request, error) {
	return nil, nil
}

const StatusOK = 200
