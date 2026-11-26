//go:build !plan9

package main
import "runtime"
func main() {
    _ = runtime.GOOS == "plan9"
}
