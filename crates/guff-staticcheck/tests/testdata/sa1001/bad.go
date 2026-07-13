package main

import "text/template"

const tmpl1 = `{{.Name}} {{.LastName}`

func main() {
	template.New("").Parse(tmpl1)
}
