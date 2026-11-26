package wrapcheck

import "example.com/ifacepkg"

func useIface(r ifacepkg.Reader) error {
	return r.Read()
}
