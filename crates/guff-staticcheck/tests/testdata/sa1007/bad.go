package main

import "net/url"

func main() {
	// One per error class net/url.Parse can return.
	url.Parse(":")
	url.Parse("cache_object:foo/bar")
	url.Parse("http://host:port/")
	url.Parse("http://host/%zz")
	url.Parse("http://h|st/")
	url.Parse("http://[::1/")
	url.Parse("http://x[::1]/")
	url.Parse("http://[12345::]/")
	url.Parse("http://us er@host/")
}
