// guff: a Rust port of Go's standard library `go/token` package.
//
// Modules:
// * [`token`] — lexical token enum, predicates, precedence.
// * [`position`] — `Pos`, `Position`, `File`, `FileSet` (port of position.go).
// * [`serialize`] — plain-data serialization mirrors of `File`/`FileSet`.
//
// The AVL tree backing `FileSet` lives in a private `tree` module.

pub mod ast;
pub mod commentmap;
pub mod constraint;
pub mod directive;
pub mod errors;
pub mod filter;
pub mod import;
pub mod parser;
pub mod parser_interface;
pub mod parser_resolver;
pub mod position;
pub mod print;
pub mod resolve;
pub mod scanner;
pub mod scope;
pub mod serialize;
pub mod stamp;
pub mod token;
pub mod walk;

mod tree;

pub use directive::{parse_directive, Directive, DirectiveArg};
pub use errors::{print_error, Error, ErrorList};
pub use position::{File, FileSet, LineInfo, Pos, Position, NO_POS};
pub use scanner::{ErrorHandler, Mode, Scanner, SCAN_COMMENTS};
pub use scope::{ObjData, ObjDecl, ObjKind, Object, Scope};
pub use serialize::{SerializedFile, SerializedFileSet};
pub use token::*;
