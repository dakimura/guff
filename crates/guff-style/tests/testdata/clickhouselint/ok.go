package clickhouselint

import (
	"context"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

var conn driver.Conn
var ctx = context.Background()

func withErr() {
	rows, _ := conn.Query(ctx, "SELECT 1")
	for rows.Next() {
	}
	_ = rows.Err()
}

func withDeferClose() {
	batch, err := conn.PrepareBatch(ctx, "INSERT INTO t")
	if err != nil {
		return
	}
	defer batch.Close()
	_ = batch.Append(1)
	_ = batch.Send()
}

func returnBatch() (driver.Batch, error) {
	batch, err := conn.PrepareBatch(ctx, "INSERT INTO t")
	if err != nil {
		return nil, err
	}
	return batch, nil
}

func deferCloseInClosure() {
	batch, err := conn.PrepareBatch(ctx, "INSERT INTO t")
	if err != nil {
		return
	}
	defer func() { _ = batch.Close() }()
}

func noNext() {
	_, _ = conn.Query(ctx, "SELECT 1")
}
