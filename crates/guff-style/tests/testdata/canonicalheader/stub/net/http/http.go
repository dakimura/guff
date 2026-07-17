package http

type Header map[string][]string

func (h Header) Get(key string) string                   { return "" }
func (h Header) Set(key, value string)                   {}
func (h Header) Add(key, value string)                   {}
func (h Header) Del(key string)                          {}
func (h Header) Values(key string) []string              { return nil }
func CanonicalHeaderKey(s string) string                 { return s }
