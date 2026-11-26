package main

import "net/url"

func main() {
	// Value-typed URL: upstream SA4027 requires *url.URL and does not flag this.
	var v url.URL
	v.Query().Set("a", "b")

	u := &url.URL{}
	q := u.Query()
	q.Set("a", "b")
	u.RawQuery = q.Encode()
}
