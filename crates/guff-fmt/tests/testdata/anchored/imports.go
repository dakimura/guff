// The import block is in reverse order. The anchored diff golangci-lint
// renders anchors on the unique line `"fmt"` and deletes `"strings"` above it
// (line 8); a minimal-edit diff instead inserts `"fmt"` above `"strings"`,
// which golangci-lint's parser would anchor on `import (` (line 7).
package anchored

import (
	"strings"
	"fmt"
)

func F() string { return fmt.Sprint(strings.ToUpper("x")) }
