package http

type Header map[string][]string

type Request struct {
	Header Header
}

func CanonicalHeaderKey(s string) string { return s }
