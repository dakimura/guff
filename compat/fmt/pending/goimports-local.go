package fixture

import (
	"fmt"

	"os"

	"github.com/other/thing"

	"example.com/mine/pkg"
)

var _ = fmt.Sprint(os.Args, pkg.X, thing.Y)
