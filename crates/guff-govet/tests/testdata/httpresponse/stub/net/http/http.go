package http

type ReadCloser interface {
	Read([]byte) (int, error)
	Close() error
}

type ResponseWriter interface {
	Write([]byte) (int, error)
}

type Request struct {
	Body ReadCloser
}

type Response struct {
	Body ReadCloser
}

type Client struct{}

func (c *Client) Get(url string) (*Response, error) {
	return nil, nil
}

func (c *Client) Do(req *Request) (*Response, error) {
	return nil, nil
}

func Get(url string) (*Response, error) {
	return nil, nil
}

func NewRequest(method, url string, body ReadCloser) (*Request, error) {
	return nil, nil
}

func MaxBytesReader(w ResponseWriter, r ReadCloser, n int64) ReadCloser {
	return nil
}
