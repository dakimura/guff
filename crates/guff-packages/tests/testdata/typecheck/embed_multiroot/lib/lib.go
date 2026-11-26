package lib

import "example.com/embedroot/ext"

// Wrapper embeds *ext.Base (cli api.HTTPError pattern).
type Wrapper struct {
	*ext.Base
}
