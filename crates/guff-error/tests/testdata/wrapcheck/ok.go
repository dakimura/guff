package wrapcheck

import (
	"encoding/json"
	"fmt"
)

func do() error {
	_, err := json.Marshal(struct{}{})
	if err != nil {
		return fmt.Errorf("marshal: %w", err)
	}
	return nil
}
