package http

type Handler interface {
	ServeHTTP(ResponseWriter, *Request)
}

type ResponseWriter interface{}
type Request struct{}
type Server struct{}
type Conn interface{}

func ListenAndServe(addr string, handler Handler) error { return nil }
func ListenAndServeTLS(addr, certFile, keyFile string, handler Handler) error { return nil }
func Serve(l interface{}, handler Handler) error { return nil }
func ServeTLS(l interface{}, handler Handler, certFile, keyFile string) error { return nil }
