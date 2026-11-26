package main

import "net/url"

func main() {
	url.Parse("foobar")
	url.Parse("https://golang.org")
	// Go reads RFC 3986: a relative reference and an opaque `scheme:path` are
	// both valid. A WHATWG parser rejects them, which is what guff used to do.
	url.Parse("mailto:a@b.c")
	url.Parse("./rel/path")
	url.Parse("*")
	// Only the last colon separates the port, so a comma-separated host list
	// parses — see compat/oracles/gourl on the `urlstrictcolons` godebug.
	url.Parse("postgres://h1:5432,h2:5433/db")
	url.Parse("http://[::1]:8080/path?q=1#frag")
	url.Parse("http://[fe80::1%25en0]/")
	// `<` `>` `"` are legal in a host: a host cannot %-encode ASCII, so
	// escaping them would amount to rejecting them outright.
	url.Parse("http://h<st/")
}
