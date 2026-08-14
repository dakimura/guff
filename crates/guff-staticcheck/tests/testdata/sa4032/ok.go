//go:build !plan9

package main
import "runtime"

const plan9 = "plan9"

func main() {
    _ = runtime.GOOS == "linux"
    _ = "plan9" == runtime.GOOS
    _ = runtime.GOOS == plan9
}
