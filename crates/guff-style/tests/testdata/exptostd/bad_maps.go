package exptostd

import "golang.org/x/exp/maps"

func use(m, a map[string]string) {
	_ = maps.Clone(m)
	maps.Equal(m, a)
	maps.Copy(m, a)
	maps.Clear(m)
	maps.DeleteFunc(m, func(_, _ string) bool { return true })
}
