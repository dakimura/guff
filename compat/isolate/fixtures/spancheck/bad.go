package p

import (
	"context"

	"go.opentelemetry.io/otel"
)

func Bad(ctx context.Context) {
	_, span := otel.Tracer("app").Start(ctx, "op")
	_ = span
}
