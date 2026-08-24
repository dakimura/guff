package p

import (
	f "fmt"
	"strings"
)

// importas has two messages: one for an import aliased to the wrong thing, one
// for an import that carries no alias where the config requires one. They read
// differently and only the first names the alias that was found.
var _ = f.Sprintf
var _ = strings.Contains
