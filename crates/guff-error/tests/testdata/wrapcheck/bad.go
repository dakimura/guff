package wrapcheck

import "encoding/json"

func do() error {
	_, err := json.Marshal(struct{}{})
	if err != nil {
		return err
	}
	return nil
}
