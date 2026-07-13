package main

import "net/url"

func main() {
	var u url.URL
	u.Query().Set("a", "b")
}
