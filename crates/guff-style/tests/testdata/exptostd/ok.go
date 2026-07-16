package exptostd

import "local/maps"

func use(m map[string]string) {
	_ = maps.Clone(m)
}
