package http

import (
	"context"
	"net/url"
	"time"
)

type Handler interface {
	ServeHTTP(ResponseWriter, *Request)
}

type Header map[string][]string

// A real method set: the ArgTypeGuards on G705's `fmt` sinks ask whether the
// writer implements this, and an empty interface is implemented by everything —
// which would silently turn the guard into a no-op.
type ResponseWriter interface {
	Header() Header
	Write([]byte) (int, error)
	WriteHeader(statusCode int)
}

type Request struct {
	Method     string
	Host       string
	RemoteAddr string
	URL        *url.URL
}

func (r *Request) Context() context.Context    { return context.Background() }
func (r *Request) FormValue(key string) string { return "" }

const StatusFound = 302

func Redirect(w ResponseWriter, r *Request, urlStr string, code int) {}
func ServeFile(w ResponseWriter, r *Request, name string)            {}
func NotFound(w ResponseWriter, r *Request)                          {}

type Conn interface{}
type FileSystem interface{}

type Server struct {
	Addr              string
	Handler           Handler
	ReadTimeout       time.Duration
	ReadHeaderTimeout time.Duration
}

type SameSite int

const (
	SameSiteDefaultMode SameSite = iota
	SameSiteLaxMode
	SameSiteStrictMode
	SameSiteNoneMode
)

type Cookie struct {
	Name     string
	Value    string
	Path     string
	Domain   string
	Secure   bool
	HttpOnly bool
	SameSite SameSite
}

func SetCookie(w ResponseWriter, cookie *Cookie) {}

func (s *Server) ListenAndServe() error { return nil }

type Response struct {
	Body   interface{ Close() error }
	Status string
}

func ListenAndServe(addr string, handler Handler) error                       { return nil }
func ListenAndServeTLS(addr, certFile, keyFile string, handler Handler) error { return nil }
func Serve(l interface{}, handler Handler) error                              { return nil }
func ServeTLS(l interface{}, handler Handler, certFile, keyFile string) error { return nil }
func Dir(root string) FileSystem                                              { return nil }
func Get(url string) (*Response, error)                                       { return nil, nil }
func Head(url string) (*Response, error)                                      { return nil, nil }
func Post(url, contentType string, body interface{}) (*Response, error)       { return nil, nil }
func PostForm(url string, data interface{}) (*Response, error)                { return nil, nil }
