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

// A second unclosed rows value is a second finding, and the message is the
// same — so this pins that the linter reports per site, not per function.
func AlsoBad() {
	rows, _ := conn.Query(context.Background(), "SELECT 2")
	for rows.Next() {
	}
}
