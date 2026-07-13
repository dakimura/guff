package http

type Header map[string][]string
func CanonicalHeaderKey(s string) string { return s }
func (Header) Set(key, value string) {}
