package main
import "encoding/json"
type t struct { x int }
func f(v t) { json.Marshal(v) }
