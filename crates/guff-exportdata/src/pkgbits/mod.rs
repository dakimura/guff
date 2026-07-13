//! Port of `golang.org/x/tools/internal/pkgbits`.

mod codes;
mod decoder;
mod reloc;
mod sync;
mod version;

pub use codes::{CodeObj, CodeType, CodeVal};
pub use decoder::{Decoder, PkgDecoder};
pub use reloc::{Index, RelocKind, PUBLIC_ROOT_IDX, RelocEnt};
pub use sync::SyncMarker;
pub use version::{Field, Version};
