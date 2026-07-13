package main
import "encoding/json"
type t struct { X int }
func f(v t) { json.Marshal(v) }
