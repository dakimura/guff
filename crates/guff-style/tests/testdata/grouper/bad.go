package grouper

import "fmt"
import "os"

const a = 1
const b = 2

var c = 3
var d = 4

type e int
type f string

func use() {
	_ = fmt.Sprint
	_ = os.Getenv
}
