// Package far reaches sumdep.Shape only through user: facts come from direct
// imports alone, and user declares no sum type, so nothing is checked here.
package far

import "example.com/sumuser/user"

func far(l user.Local) {
	switch l.(type) {
	}
}
