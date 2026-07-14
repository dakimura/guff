package ok

import "fmt"

func okPath(host string) string {
	return fmt.Sprintf("http://%s/path", host)
}

func okLiteralHost() string {
	return fmt.Sprintf("http://%s:8080", "localhost")
}

func okMessage(name string) string {
	return fmt.Sprintf("hello %s", name)
}
