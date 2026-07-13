//! `guff-ssa` — a Rust port of Go's `golang.org/x/tools/go/ssa`.
//!
//! Static single-assignment (SSA) form intermediate representation for the
//! bodies of Go functions. Built on top of the type information produced by
//! `guff-types` (the arena-based port of `go/types`).
//!
//! Design mirrors `guff-types`: SSA objects live in arenas addressed by
//! `NonZeroU32` id handles (`FuncId`/`BlockId`/`ValueId`/`InstrId`) rather than
//! trait objects or `Rc<RefCell<..>>`. Type information is carried around as
//! `guff_types` `TypeId`/`ObjectId` and borrowed read-only.
//!
//! See `projects/guff-ssa-MIGRATION.md` for the porting roadmap.

// Modules are added chunk by chunk (see MIGRATION.md §4).
pub mod arena;
pub mod block;
pub mod blockopt;
pub mod builder;
pub mod canon;
pub mod const_val;
pub mod create;
pub mod dom;
pub mod emit;
pub mod function;
pub mod global;
pub mod has_params;
pub mod ids;
pub mod instantiate;
pub mod instr;
pub mod lift;
pub mod lvalue;
pub mod member;
pub mod methods;
pub mod mode;
pub mod node;
pub mod package;
pub mod print;
pub mod program;
pub mod sanity;
pub mod source;
pub mod ssautil;
pub mod subst;
pub mod typeset;
pub mod wrappers;
pub mod value;

pub use arena::{Arena, ArenaId};
pub use block::BasicBlock;
pub use const_val::Const;
pub use function::{Function, Parameter, FreeVar};
pub use global::Global;
pub use member::MemberData;
pub use ids::{
    BlockId, BuiltinId, ConstId, FreeVarId, FuncId, GlobalId, InstrId, PackageId, ParamId,
};
pub use mode::BuilderMode;
pub use node::{Instruction, Member, Node, Value as ValueTrait};
pub use package::Package;
pub use program::Program;
pub use value::Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_program_structure() {
        let prog = Program::new(
            BuilderMode::default(),
            guff_types::Info::default(),
            guff_types::TypeArena::new(),
            guff_types::ObjectArena::new(),
            guff_types::PackageArena::new(),
        );
        assert_eq!(prog.packages.len(), 0);
        assert_eq!(prog.functions.len(), 0);
    }

    #[test]
    fn test_function_block_allocation() {
        let mut fn_obj = Function::new("test".to_string(), None, None);
        let block_id = fn_obj.blocks.alloc(BasicBlock::new(0, FuncId::from_index(0)));
        assert_eq!(fn_obj.blocks.len(), 1);
        assert_eq!(fn_obj.blocks.get(block_id).index, 0);
    }

    #[test]
    fn test_create_phase() {
        use std::num::NonZeroU32;

        let mut prog = Program::new(
            BuilderMode::default(),
            guff_types::Info::default(),
            guff_types::TypeArena::new(),
            guff_types::ObjectArena::new(),
            guff_types::PackageArena::new(),
        );
        let type_pkg_id = unsafe { std::mem::transmute::<NonZeroU32, guff_types::PackageId>(NonZeroU32::new(1).unwrap()) };
        let pkg_id = create::create_package(&mut prog, type_pkg_id);
        
        let fn_id = create::create_function(&mut prog, "main".to_string(), None, Some(pkg_id));

        assert_eq!(prog.packages.len(), 1);
        // create_package synthesizes the package initializer `init`, so the
        // program holds it plus the explicitly created `main`.
        assert_eq!(prog.functions.len(), 2);
        assert_eq!(prog.functions.get(fn_id).name, "main");
        let init_fid = prog.packages.get(pkg_id).init.expect("init synthesized");
        assert_eq!(prog.functions.get(init_fid).name, "init");
        assert_eq!(
            prog.functions.get(init_fid).synthetic.as_deref(),
            Some("package initializer")
        );
    }
}
