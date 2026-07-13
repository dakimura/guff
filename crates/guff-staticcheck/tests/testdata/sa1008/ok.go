package main

import "net/http"

func main() {
	h := http.Header{}
	_ = h["Foo"]
	var s []string
	h["Foo"] = s
	var m map[string][]string
	_ = m["foo"]
}
