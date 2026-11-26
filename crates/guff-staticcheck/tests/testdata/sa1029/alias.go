package sa1029alias

import "context"

type aliasEmpty = struct{}
type namedEmpty struct{}
type aliasString = string
type namedString string
type aliasInt = int
type aliasNonEmpty = struct{ x int }
type aliasOfAlias = aliasEmpty

// Keys whose type is, or aliases, an empty struct or a basic type. Upstream
// unaliases for the basic-type message and not for the empty-struct one.
func aliases(ctx context.Context) {
	_ = context.WithValue(ctx, struct{}{}, 1)       // reported: empty anonymous struct
	_ = context.WithValue(ctx, aliasEmpty{}, 1)     // silent: an alias of struct{} (lima)
	_ = context.WithValue(ctx, namedEmpty{}, 1)     // silent: a named type
	_ = context.WithValue(ctx, "k", 1)              // reported: built-in type string
	_ = context.WithValue(ctx, aliasString("k"), 1) // reported: string via alias aliasString
	_ = context.WithValue(ctx, namedString("k"), 1) // silent: a named type
	_ = context.WithValue(ctx, aliasInt(1), 1)      // reported: int via alias aliasInt
	_ = context.WithValue(ctx, aliasNonEmpty{1}, 1) // silent: not empty
	_ = context.WithValue(ctx, aliasOfAlias{}, 1)   // silent: an alias of an alias of struct{}
	_ = context.WithValue(ctx, 1, 1)                // reported: built-in type int
}
