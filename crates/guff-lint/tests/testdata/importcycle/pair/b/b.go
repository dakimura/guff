// Package b imports a, which imports b.
package b

import "example.com/cycle/a"

// B is never type-checked: the package does not load.
var B = len(a.Name)
