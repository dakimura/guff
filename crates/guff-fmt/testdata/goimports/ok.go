package ok

// Already grouped the way `local-prefixes` asks for, so this file reports
// nothing — the control that keeps the case from being green by silence.

import (
	"fmt"

	"third.example/pkg"

	"zlocal.example/local"
)

var _ = fmt.Sprint
var _ = pkg.Name
var _ = local.Name
