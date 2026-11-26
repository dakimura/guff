//! Approximate retained-heap accounting for RSS attribution (PERF_TASKS_V2 C-8).
//!
//! Arena slot charges live on the arena types (they need private `Layered`
//! access). This module holds the accumulator, `Info` map estimates, and the
//! package-level helper used by guff-packages / guff-lint.

use std::mem::size_of;
use std::sync::Arc;

use crate::api::{Info, TypeAndValue};
use crate::arena::{ObjectArena, PackageArena, ScopeArena, TypeArena};
use crate::hash::HashSet;
use crate::selection::Selection;

/// Accumulator for unique retained bytes across shared arena clones.
#[derive(Debug, Default)]
pub struct RetainedBytes {
    pub(crate) seen_ptrs: HashSet<usize>,
    /// `TypeData` / `ObjectData` / … slot storage (deduped Arc bases + overlays).
    pub type_slots: usize,
    pub object_slots: usize,
    pub scope_slots: usize,
    pub package_slots: usize,
    /// Heap string bytes hanging off objects / type packages (names, paths).
    pub name_bytes: usize,
    /// Intern-table estimate (keys + values), Arc-deduped.
    pub intern_tables: usize,
    /// `Info` map entry estimates (Arc-deduped).
    pub info_maps: usize,
}

impl RetainedBytes {
    pub fn types_total(&self) -> usize {
        self.type_slots
            + self.object_slots
            + self.scope_slots
            + self.package_slots
            + self.name_bytes
            + self.intern_tables
    }

    pub fn attributed_total(&self) -> usize {
        self.types_total() + self.info_maps
    }
}

pub(crate) fn account_arc_vec_slots<T>(
    seen: &mut HashSet<usize>,
    arc: &Arc<Vec<T>>,
    overlay_cap: usize,
    slots: &mut usize,
) -> bool {
    let ptr = Arc::as_ptr(arc) as usize;
    let first = seen.insert(ptr);
    if first {
        *slots = slots.saturating_add(arc.capacity().saturating_mul(size_of::<T>()));
    }
    *slots = slots.saturating_add(overlay_cap.saturating_mul(size_of::<T>()));
    first
}

pub(crate) fn account_arc_map_approx<K, V>(
    seen: &mut HashSet<usize>,
    arc: &Arc<crate::hash::HashMap<K, V>>,
    out: &mut usize,
) {
    let ptr = Arc::as_ptr(arc) as usize;
    if seen.insert(ptr) {
        *out = out.saturating_add(
            arc.len()
                .saturating_mul(size_of::<K>().saturating_add(size_of::<V>()).saturating_add(16)),
        );
    }
}

pub(crate) fn account_owned_map_approx<K, V>(map: &crate::hash::HashMap<K, V>, out: &mut usize) {
    *out = out.saturating_add(
        map.len()
            .saturating_mul(size_of::<K>().saturating_add(size_of::<V>()).saturating_add(16)),
    );
}

/// Approximate bytes for an [`Info`] map bundle (Arc-deduped).
pub fn account_info(info: &Arc<Info>, acct: &mut RetainedBytes) {
    let ptr = Arc::as_ptr(info) as usize;
    if !acct.seen_ptrs.insert(ptr) {
        return;
    }
    let mut n = 0usize;
    account_owned_map_approx(&info.types, &mut n);
    let tv_extra = size_of::<TypeAndValue>().saturating_sub(8);
    n = n.saturating_add(info.types.len().saturating_mul(tv_extra));
    account_owned_map_approx(&info.defs, &mut n);
    account_owned_map_approx(&info.uses, &mut n);
    account_owned_map_approx(&info.selections, &mut n);
    n = n.saturating_add(info.selections.len().saturating_mul(size_of::<Selection>() / 2));
    account_owned_map_approx(&info.instances, &mut n);
    account_owned_map_approx(&info.scopes, &mut n);
    account_owned_map_approx(&info.implicits, &mut n);
    n = n.saturating_add(info.init_order.capacity().saturating_mul(64));
    acct.info_maps = acct.info_maps.saturating_add(n);
}

/// Convenience: account a full typecheck artifact bundle.
pub fn account_typecheck_arenas(
    types: &TypeArena,
    objects: &ObjectArena,
    scopes: &ScopeArena,
    packages: &PackageArena,
    info: &Arc<Info>,
    acct: &mut RetainedBytes,
) {
    types.account_retained(acct);
    objects.account_retained(acct);
    scopes.account_retained(acct);
    packages.account_retained(acct);
    account_info(info, acct);
}
