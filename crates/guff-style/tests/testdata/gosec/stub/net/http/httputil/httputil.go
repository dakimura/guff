package httputil

import "net/url"

type ReverseProxy struct{}

func NewSingleHostReverseProxy(target *url.URL) *ReverseProxy { return &ReverseProxy{} }
