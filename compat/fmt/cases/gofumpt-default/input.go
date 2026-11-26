package fixture

import (
	"fmt"

	"os"
)

func demo() {

	x := 1

	if x == 1 {
		fmt.Println(os.Args)
	}

}

var s = []string{
	"a",
	"b",
}
