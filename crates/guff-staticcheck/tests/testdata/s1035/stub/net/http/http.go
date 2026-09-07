package http

type Header map[string][]string

func CanonicalHeaderKey(s string) string { return s }

func (Header) Add(key, value string)  {}
func (Header) Del(key string)         {}
func (Header) Get(key string) string  { return "" }
func (Header) Set(key, value string)  {}
