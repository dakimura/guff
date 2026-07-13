package main

import "net/url"

func main() {
	url.Parse("foobar")
	url.Parse("https://golang.org")
}
