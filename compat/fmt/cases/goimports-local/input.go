package fixture

import (
	"example.com/mine/pkg"
	"fmt"
	"github.com/other/thing"
	"os"
)

var _ = fmt.Sprint(os.Args, pkg.X, thing.Y)
