package importas_bad

import f "fmt"
import wrong "os"

// The use sites matter as much as the import lines: renaming the alias without
// renaming these leaves `f` and `wrong` undefined.
var _ = f.Sprintf
var _ = wrong.Getenv
