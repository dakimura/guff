package noctx

import (
	"context"
	"net/http"
)

func ok() {
	ctx := context.Background()
	req, _ := http.NewRequestWithContext(ctx, "GET", "https://example.com", nil)
	_ = req
}
