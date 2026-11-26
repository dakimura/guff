package gosec_settings

import "crypto/md5"

func onlyImport() {
	_ = md5.New()
}
