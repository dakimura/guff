// SA4004 examines every loop in a function, not the ones written at its top
// level.
//
// Upstream's walk is `ast.Inspect(body, …)` over the whole body; guff looked
// only at the function body's own statements, so a loop pair inside an `if`
// was never examined. opentofu's
// `internal/legacy/helper/schema/resource_timeout.go:144` is that shape —
// `if raw, ok := c.Config[TimeoutsConfigKey]; ok {` around the loops.
//
// The labelled shapes are the other half of the port: upstream's test is
// `stmt.Label == nil || labels[ObjectOf(stmt.Label)] == loop`, so a labelled
// `break` naming *this* loop is an unconditional exit, while a labelled
// `continue` naming an enclosing one is not the loop's own continue.
package main

func insideIf(ok bool, rows [][]int) error {
	// fires — the body returns, and the inner loop is the branching statement
	// that upstream requires before it reports.
	if ok {
		for _, vals := range rows {
			for _, v := range vals {
				_ = v
			}

			return nil
		}
	}

	return nil
}

func insideBareBlock(rows [][]int) error {
	// fires — any block will do; there is nothing special about `if`.
	{
		for _, vals := range rows {
			for _, v := range vals {
				_ = v
			}

			return nil
		}
	}

	return nil
}

func labelledBreak(rows [][]int) int {
	// fires — `break outer` names this loop.
outer:
	for _, vals := range rows {
		if len(vals) > 0 {
			_ = vals
		}
		break outer
	}

	return 0
}

func labelledContinueOuter(rows [][]int) int {
	// silent — `continue outer` names this loop, so it is not terminated.
outer:
	for _, vals := range rows {
		for _, v := range vals {
			if v < 0 {
				continue outer
			}
		}

		return 1
	}

	return 0
}

func gotoCancels(rows [][]int) int {
	// silent — a `goto` anywhere in the body cancels the finding.
	for _, vals := range rows {
		if len(vals) == 0 {
			goto end
		}

		return 1
	}
end:

	return 0
}

func nestedLoopInsideASilentOne(rows [][]int) int {
	// silent — a one-statement range loop is the "first element" pattern and
	// upstream keeps walking into it, where the inner loop is a finding.
	for _, vals := range rows {
		for _, v := range vals {
			if v < 0 {
				_ = v
			}

			return v
		}
	}

	return 0
}
