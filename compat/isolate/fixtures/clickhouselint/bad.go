package p

import (
	"context"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

var conn driver.Conn

func Bad() {
	rows, _ := conn.Query(context.Background(), "SELECT 1")
	for rows.Next() {
	}
}
