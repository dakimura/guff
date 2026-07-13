//go:build linux

package main
import "runtime"
func main() {
    _ = runtime.GOOS == "windows"
}
