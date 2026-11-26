package main
import "os"
func f() { os.RemoveAll(os.TempDir() + "/sub") }
