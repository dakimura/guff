package main
import "net/url"
func main() {
    u := &url.URL{}
    q := u.Query()
    q.Set("a", "b")
    u.RawQuery = q.Encode()
}
