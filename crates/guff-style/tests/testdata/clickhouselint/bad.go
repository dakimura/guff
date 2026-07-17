package clickhouselint

import (
	"context"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

var conn driver.Conn
var ctx = context.Background()

func missingErr() {
	rows, _ := conn.Query(ctx, "SELECT 1")
	for rows.Next() {
	}
}

func missingBatchClose() {
	batch, err := conn.PrepareBatch(ctx, "INSERT INTO t")
	if err != nil {
		return
	}
	_ = batch.Append(1)
	_ = batch.Send()
}

func blankBatch() {
	_, err := conn.PrepareBatch(ctx, "INSERT INTO t")
	if err != nil {
		return
	}
}

func deferAbortOnly() {
	batch, err := conn.PrepareBatch(ctx, "INSERT INTO t")
	if err != nil {
		return
	}
	defer batch.Abort()
	_ = batch.Send()
}
