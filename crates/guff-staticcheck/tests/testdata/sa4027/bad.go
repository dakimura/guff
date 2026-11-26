package main

import "net/url"

func main() {
	u := &url.URL{}
	u.Query().Set("a", "b")
}
