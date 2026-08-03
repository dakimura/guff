package inline_ioutil

import "io/ioutil"

func useTempDir() {
	_, _ = ioutil.TempDir("", "")
}
