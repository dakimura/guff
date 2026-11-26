package http

type Header map[string][]string

func (h Header) Get(key string) string      { return "" }
func (h Header) Set(key, value string)      {}
func (h Header) Add(key, value string)      {}
func (h Header) Del(key string)             {}
func (h Header) Values(key string) []string { return nil }
func CanonicalHeaderKey(s string) string    { return s }

// `Header` is the name of four objects in net/http, and upstream's
// `headerObject` scan keeps whichever one the `Uses` map hands it first. Two of
// them carry the type itself; the method does not, and that is the difference
// the fixture measures.
type Request struct {
	Header Header
}

type ResponseWriter interface {
	Header() Header
}
