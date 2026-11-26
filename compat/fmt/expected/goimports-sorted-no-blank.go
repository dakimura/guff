package fixture

import (
	"fmt"
	"os"

	"github.com/other/thing"
)

var _ = fmt.Sprint(os.Args, thing.Y)
