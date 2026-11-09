// Not upstream testdata: wrapcheck matches *ast.CallExpr and *ast.Ident without
// unwrapping parentheses, so a parenthesised return value ends the statement.
package main

import "encoding/json"

func main() {}

func parenCall() error {
	return (json.Unmarshal(nil, nil))
}

func parenIdent() error {
	err := json.Unmarshal(nil, nil)
	return (err)
}

func parenRHS() error {
	err := (json.Unmarshal(nil, nil))
	return err
}

func plain() error {
	err := json.Unmarshal(nil, nil)
	return err
}
