/*
Package blockdoc writes its package doc as a block comment, which is what
cloud.google.com/go/pubsub does. `CommentGroup.Text()` strips the delimiters,
so the last paragraph is an ordinary `Deprecated:` one — but it carries no
marker of its own in the source, and the byte probe that decides whether to
parse the file at all only knew `// Deprecated:` and `* Deprecated:`.

Deprecated: Please use example.com/old.
*/
package blockdoc

// H does nothing.
func H() {}
