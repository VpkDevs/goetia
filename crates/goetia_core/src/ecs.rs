//! Archetype-based ECS. Entities are generational IDs; components live in
//! contiguous per-archetype columns; queries iterate raw slices.
//!
//! No per-entity allocation on the hot path: spawning into an existing
//! archetype is a handful of Vec pushes.

use crate::hash::FnvHashMap;
use std::any::{Any, TypeId};
use std::cell::UnsafeCell;

// ---------------------------------------------------------------- entities

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Entity {
    pub index: u32,
    pub gen: u32,
}

impl Entity {
    pub const DEAD: Entity = Entity {
        index: u32::MAX,
        gen: u32::MAX,
    };
    pub fn to_bits(self) -> u64 {
        ((self.gen as u64) << 32) | self.index as u64
    }
    pub fn from_bits(b: u64) -> Self {
        Entity {
            index: b as u32,
            gen: (b >> 32) as u32,
        }
    }
}

#[derive(Clone, Copy)]
struct Meta {
    gen: u32,
    arch: u32,
    row: u32,
    alive: bool,
}

// ---------------------------------------------------------------- columns

/// Constructor for a type-erased component column.
pub type ColumnCtor = fn() -> Box<dyn Column>;
/// A deferred structural edit applied to the world later.
type WorldOp = Box<dyn FnOnce(&mut World) + Send>;
/// Extra initialization run on the destination archetype during migration.
type ArchetypeInit = Box<dyn FnOnce(&mut Archetype)>;

#[doc(hidden)]
// `len` here is a column-length accessor on a type-erased internal trait; an
// `is_empty` counterpart would be dead weight.
#[allow(clippy::len_without_is_empty)]
pub trait Column: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    #[allow(dead_code)]
    fn len(&self) -> usize;
    fn swap_remove_drop(&mut self, row: usize);
    /// swap_remove `row` from self and push the value onto `target` (same T).
    fn move_row(&mut self, row: usize, target: &mut dyn Column);
    fn new_empty(&self) -> Box<dyn Column>;
}

struct Col<T: 'static + Send + Sync>(UnsafeCell<Vec<T>>);
unsafe impl<T: 'static + Send + Sync> Send for Col<T> {}
unsafe impl<T: 'static + Send + Sync> Sync for Col<T> {}

impl<T: 'static + Send + Sync> Col<T> {
    fn vec_mut(&mut self) -> &mut Vec<T> {
        self.0.get_mut()
    }
}

impl<T: 'static + Send + Sync> Column for Col<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn len(&self) -> usize {
        unsafe { (*self.0.get()).len() }
    }
    fn swap_remove_drop(&mut self, row: usize) {
        self.vec_mut().swap_remove(row);
    }
    fn move_row(&mut self, row: usize, target: &mut dyn Column) {
        let v = self.vec_mut().swap_remove(row);
        target
            .as_any_mut()
            .downcast_mut::<Col<T>>()
            .unwrap()
            .vec_mut()
            .push(v);
    }
    fn new_empty(&self) -> Box<dyn Column> {
        Box::new(Col::<T>(UnsafeCell::new(Vec::new())))
    }
}

fn new_col<T: 'static + Send + Sync>() -> Box<dyn Column> {
    Box::new(Col::<T>(UnsafeCell::new(Vec::new())))
}

// ---------------------------------------------------------------- archetype

pub struct Archetype {
    sig: Box<[TypeId]>, // sorted
    cols: Vec<Box<dyn Column>>,
    entities: Vec<Entity>,
}

impl Archetype {
    fn col_index(&self, tid: TypeId) -> Option<usize> {
        self.sig.binary_search(&tid).ok()
    }
    pub fn contains(&self, tid: TypeId) -> bool {
        self.col_index(tid).is_some()
    }
    pub fn len(&self) -> usize {
        self.entities.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    fn col<T: 'static + Send + Sync>(&self) -> Option<&Col<T>> {
        let i = self.col_index(TypeId::of::<T>())?;
        self.cols[i].as_any().downcast_ref::<Col<T>>()
    }
    fn col_mut<T: 'static + Send + Sync>(&mut self) -> Option<&mut Col<T>> {
        let i = self.col_index(TypeId::of::<T>())?;
        self.cols[i].as_any_mut().downcast_mut::<Col<T>>()
    }

    /// Raw data pointer to the column for `T`.
    ///
    /// # Safety
    /// The caller must uphold aliasing rules: either hold exclusive access to
    /// the world, or run under the scheduler, which proves that concurrently
    /// executing systems have disjoint component read/write sets. Returns a
    /// dangling-if-empty pointer valid for `Archetype::len()` elements.
    pub unsafe fn col_ptr<T: 'static + Send + Sync>(&self) -> *mut T {
        let c = self.col::<T>().expect("archetype missing column");
        (*c.0.get()).as_mut_ptr()
    }

    pub fn push_value<T: 'static + Send + Sync>(&mut self, v: T) {
        self.col_mut::<T>()
            .expect("bundle wrote unknown component")
            .vec_mut()
            .push(v);
    }
}

// ---------------------------------------------------------------- bundles

pub trait Bundle: 'static {
    fn types(out: &mut Vec<(TypeId, ColumnCtor)>);
    fn write(self, arch: &mut Archetype);
}

macro_rules! impl_bundle {
    ($($t:ident.$idx:tt),+) => {
        impl<$($t: 'static + Send + Sync),+> Bundle for ($($t,)+) {
            fn types(out: &mut Vec<(TypeId, ColumnCtor)>) {
                $(out.push((TypeId::of::<$t>(), new_col::<$t>));)+
            }
            fn write(self, arch: &mut Archetype) {
                $(arch.push_value(self.$idx);)+
            }
        }
    };
}
impl_bundle!(A.0);
impl_bundle!(A.0, B.1);
impl_bundle!(A.0, B.1, C.2);
impl_bundle!(A.0, B.1, C.2, D.3);
impl_bundle!(A.0, B.1, C.2, D.3, E.4);
impl_bundle!(A.0, B.1, C.2, D.3, E.4, F.5);
impl_bundle!(A.0, B.1, C.2, D.3, E.4, F.5, G.6);
impl_bundle!(A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7);
impl_bundle!(A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7, I.8);
impl_bundle!(A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7, I.8, J.9);
impl_bundle!(A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7, I.8, J.9, K.10);
impl_bundle!(A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7, I.8, J.9, K.10, L.11);

// ---------------------------------------------------------------- queries

/// One element of a query tuple: `&T` or `&mut T`.
pub trait Query<'w>: Sized {
    type Item;
    type Ptr: Copy;
    fn collect(out: &mut Vec<(TypeId, bool)>);
    fn matches(arch: &Archetype) -> bool;
    /// # Safety
    /// Aliasing discipline is upheld by the caller; see [`Archetype::col_ptr`].
    unsafe fn ptr(arch: &Archetype) -> Self::Ptr;
    /// # Safety
    /// `row` must be `< Archetype::len()` for the archetype `p` came from, and
    /// the aliasing contract of [`Query::ptr`] must still hold.
    unsafe fn get(p: Self::Ptr, row: usize) -> Self::Item;
}

impl<'w, T: 'static + Send + Sync> Query<'w> for &'w T {
    type Item = &'w T;
    type Ptr = *mut T;
    fn collect(out: &mut Vec<(TypeId, bool)>) {
        out.push((TypeId::of::<T>(), false));
    }
    fn matches(arch: &Archetype) -> bool {
        arch.contains(TypeId::of::<T>())
    }
    unsafe fn ptr(arch: &Archetype) -> *mut T {
        arch.col_ptr::<T>()
    }
    unsafe fn get(p: *mut T, row: usize) -> &'w T {
        &*p.add(row)
    }
}

impl<'w, T: 'static + Send + Sync> Query<'w> for &'w mut T {
    type Item = &'w mut T;
    type Ptr = *mut T;
    fn collect(out: &mut Vec<(TypeId, bool)>) {
        out.push((TypeId::of::<T>(), true));
    }
    fn matches(arch: &Archetype) -> bool {
        arch.contains(TypeId::of::<T>())
    }
    unsafe fn ptr(arch: &Archetype) -> *mut T {
        arch.col_ptr::<T>()
    }
    unsafe fn get(p: *mut T, row: usize) -> &'w mut T {
        &mut *p.add(row)
    }
}

macro_rules! impl_query_tuple {
    ($($t:ident),+) => {
        impl<'w, $($t: Query<'w>),+> Query<'w> for ($($t,)+) {
            type Item = ($($t::Item,)+);
            type Ptr = ($($t::Ptr,)+);
            fn collect(out: &mut Vec<(TypeId, bool)>) {
                $($t::collect(out);)+
            }
            fn matches(arch: &Archetype) -> bool {
                $($t::matches(arch))&&+
            }
            unsafe fn ptr(arch: &Archetype) -> Self::Ptr {
                ($($t::ptr(arch),)+)
            }
            unsafe fn get(p: Self::Ptr, row: usize) -> Self::Item {
                #[allow(non_snake_case)]
                let ($($t,)+) = p;
                ($($t::get($t, row),)+)
            }
        }
    };
}
impl_query_tuple!(A);
impl_query_tuple!(A, B);
impl_query_tuple!(A, B, C);
impl_query_tuple!(A, B, C, D);
impl_query_tuple!(A, B, C, D, E);
impl_query_tuple!(A, B, C, D, E, F);
impl_query_tuple!(A, B, C, D, E, F, G);
impl_query_tuple!(A, B, C, D, E, F, G, H);

// ---------------------------------------------------------------- world

#[derive(Default)]
pub struct World {
    metas: Vec<Meta>,
    free: Vec<u32>,
    archetypes: Vec<Archetype>,
    arch_index: FnvHashMap<Box<[TypeId]>, u32>,
    resources: FnvHashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_or_create_archetype(&mut self, mut types: Vec<(TypeId, ColumnCtor)>) -> u32 {
        types.sort_by_key(|(t, _)| *t);
        debug_assert!(
            types.windows(2).all(|w| w[0].0 != w[1].0),
            "bundle contains duplicate component types"
        );
        let sig: Box<[TypeId]> = types.iter().map(|(t, _)| *t).collect();
        if let Some(&i) = self.arch_index.get(&sig) {
            return i;
        }
        let arch = Archetype {
            sig: sig.clone(),
            cols: types.iter().map(|(_, f)| f()).collect(),
            entities: Vec::new(),
        };
        let i = self.archetypes.len() as u32;
        self.archetypes.push(arch);
        self.arch_index.insert(sig, i);
        i
    }

    fn alloc_entity(&mut self) -> Entity {
        if let Some(index) = self.free.pop() {
            let m = &mut self.metas[index as usize];
            m.alive = true;
            Entity { index, gen: m.gen }
        } else {
            let index = self.metas.len() as u32;
            self.metas.push(Meta {
                gen: 0,
                arch: 0,
                row: 0,
                alive: true,
            });
            Entity { index, gen: 0 }
        }
    }

    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let mut types = Vec::new();
        B::types(&mut types);
        let ai = self.get_or_create_archetype(types);
        let e = self.alloc_entity();
        let arch = &mut self.archetypes[ai as usize];
        let row = arch.entities.len() as u32;
        arch.entities.push(e);
        bundle.write(arch);
        let m = &mut self.metas[e.index as usize];
        m.arch = ai;
        m.row = row;
        e
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        self.metas
            .get(e.index as usize)
            .map(|m| m.alive && m.gen == e.gen)
            .unwrap_or(false)
    }

    /// Remove entity from its archetype (swap-remove all columns), fixing up
    /// the row index of whichever entity got swapped into its slot.
    fn detach(&mut self, e: Entity) -> u32 {
        let m = self.metas[e.index as usize];
        let arch = &mut self.archetypes[m.arch as usize];
        let row = m.row as usize;
        for c in &mut arch.cols {
            c.swap_remove_drop(row);
        }
        arch.entities.swap_remove(row);
        if let Some(&moved) = arch.entities.get(row) {
            self.metas[moved.index as usize].row = row as u32;
        }
        m.arch
    }

    pub fn despawn(&mut self, e: Entity) -> bool {
        if !self.is_alive(e) {
            return false;
        }
        self.detach(e);
        let m = &mut self.metas[e.index as usize];
        m.alive = false;
        m.gen = m.gen.wrapping_add(1);
        self.free.push(e.index);
        true
    }

    pub fn get<T: 'static + Send + Sync>(&self, e: Entity) -> Option<&T> {
        if !self.is_alive(e) {
            return None;
        }
        let m = self.metas[e.index as usize];
        let arch = &self.archetypes[m.arch as usize];
        let c = arch.col::<T>()?;
        unsafe { (&*c.0.get()).get(m.row as usize) }
    }

    pub fn get_mut<T: 'static + Send + Sync>(&mut self, e: Entity) -> Option<&mut T> {
        if !self.is_alive(e) {
            return None;
        }
        let m = self.metas[e.index as usize];
        let arch = &mut self.archetypes[m.arch as usize];
        let c = arch.col_mut::<T>()?;
        c.vec_mut().get_mut(m.row as usize)
    }

    pub fn has<T: 'static + Send + Sync>(&self, e: Entity) -> bool {
        self.get::<T>(e).is_some()
    }

    /// Add (or overwrite) a component, migrating the entity across archetypes
    /// if needed.
    pub fn insert<T: 'static + Send + Sync>(&mut self, e: Entity, value: T) {
        if !self.is_alive(e) {
            return;
        }
        if let Some(slot) = self.get_mut::<T>(e) {
            *slot = value;
            return;
        }
        let m = self.metas[e.index as usize];
        let src_i = m.arch as usize;
        // Target signature = source + T; clone columns structurally via new_empty.
        let mut sig: Vec<TypeId> = self.archetypes[src_i].sig.to_vec();
        sig.push(TypeId::of::<T>());
        sig.sort();
        let sig: Box<[TypeId]> = sig.into();
        let dst_i = if let Some(&i) = self.arch_index.get(&sig) {
            i
        } else {
            let src = &self.archetypes[src_i];
            let mut cols: Vec<(TypeId, Box<dyn Column>)> = src
                .sig
                .iter()
                .zip(&src.cols)
                .map(|(t, c)| (*t, c.new_empty()))
                .collect();
            cols.push((TypeId::of::<T>(), new_col::<T>()));
            cols.sort_by_key(|(t, _)| *t);
            let arch = Archetype {
                sig: sig.clone(),
                cols: cols.into_iter().map(|(_, c)| c).collect(),
                entities: Vec::new(),
            };
            let i = self.archetypes.len() as u32;
            self.archetypes.push(arch);
            self.arch_index.insert(sig, i);
            i
        };
        self.migrate(
            e,
            src_i as u32,
            dst_i,
            Some(Box::new(move |arch: &mut Archetype| {
                arch.push_value(value);
            })),
        );
    }

    /// Remove a component, migrating the entity to the reduced archetype.
    pub fn remove<T: 'static + Send + Sync>(&mut self, e: Entity) {
        if !self.is_alive(e) || !self.has::<T>(e) {
            return;
        }
        let m = self.metas[e.index as usize];
        let src_i = m.arch as usize;
        let removed = TypeId::of::<T>();
        let sig: Box<[TypeId]> = self.archetypes[src_i]
            .sig
            .iter()
            .copied()
            .filter(|t| *t != removed)
            .collect();
        let dst_i = if let Some(&i) = self.arch_index.get(&sig) {
            i
        } else {
            let src = &self.archetypes[src_i];
            let cols = src
                .sig
                .iter()
                .zip(&src.cols)
                .filter(|(t, _)| **t != removed)
                .map(|(_, c)| c.new_empty())
                .collect();
            let arch = Archetype {
                sig: sig.clone(),
                cols,
                entities: Vec::new(),
            };
            let i = self.archetypes.len() as u32;
            self.archetypes.push(arch);
            self.arch_index.insert(sig, i);
            i
        };
        self.migrate(e, src_i as u32, dst_i, None);
    }

    /// Move entity between archetypes, transplanting shared columns.
    fn migrate(&mut self, e: Entity, src_i: u32, dst_i: u32, extra: Option<ArchetypeInit>) {
        debug_assert_ne!(src_i, dst_i);
        let row = self.metas[e.index as usize].row as usize;
        let (src, dst) = {
            let (a, b) = if src_i < dst_i {
                let (l, r) = self.archetypes.split_at_mut(dst_i as usize);
                (&mut l[src_i as usize], &mut r[0])
            } else {
                let (l, r) = self.archetypes.split_at_mut(src_i as usize);
                (&mut r[0], &mut l[dst_i as usize])
            };
            (a, b)
        };
        // Move shared columns (dst sig may add or drop types vs src).
        for (i, t) in src.sig.clone().iter().enumerate() {
            if let Some(j) = dst.col_index(*t) {
                // Split borrows of the two Vec<Box<dyn Column>> live in
                // different archetypes, so this is fine.
                src.cols[i].move_row(row, &mut *dst.cols[j]);
            } else {
                src.cols[i].swap_remove_drop(row);
            }
        }
        src.entities.swap_remove(row);
        dst.entities.push(e);
        if let Some(f) = extra {
            f(dst);
        }
        let dst_row = (dst.entities.len() - 1) as u32;
        if let Some(&moved) = src.entities.get(row) {
            self.metas[moved.index as usize].row = row as u32;
        }
        let m = &mut self.metas[e.index as usize];
        m.arch = dst_i;
        m.row = dst_row;
    }

    // ------------------------------------------------------------ iteration

    /// Iterate all entities matching query tuple `Q` (elements are `&T` /
    /// `&mut T`). Exclusive `&mut self` makes aliasing safe for a single call;
    /// parallel system execution is validated by the scheduler's access sets.
    pub fn each<'w, Q: Query<'w>>(&'w mut self, mut f: impl FnMut(Entity, Q::Item)) {
        unsafe { self.each_unchecked::<Q>(&mut f) }
    }

    /// Iterate without the exclusive-borrow requirement of [`World::each`].
    ///
    /// # Safety
    /// The caller must guarantee that no other thread holds aliasing mutable
    /// access to the same component columns for the duration of the call.
    /// The scheduler upholds this by batching only systems with disjoint
    /// write sets.
    pub unsafe fn each_unchecked<'w, Q: Query<'w>>(&'w self, f: &mut impl FnMut(Entity, Q::Item)) {
        for arch in &self.archetypes {
            if arch.entities.is_empty() || !Q::matches(arch) {
                continue;
            }
            let p = Q::ptr(arch);
            let n = arch.entities.len();
            for row in 0..n {
                f(*arch.entities.get_unchecked(row), Q::get(p, row));
            }
        }
    }

    /// Count entities matching the query.
    pub fn count<'w, Q: Query<'w>>(&'w mut self) -> usize {
        let mut n = 0;
        for arch in &self.archetypes {
            if !arch.entities.is_empty() && Q::matches(arch) {
                n += arch.entities.len();
            }
        }
        n
    }

    pub fn entity_count(&self) -> usize {
        self.metas.len() - self.free.len()
    }

    pub fn archetype_stats(&self) -> Vec<(usize, usize)> {
        // (component count, entity count) per archetype — for the debug overlay.
        self.archetypes
            .iter()
            .map(|a| (a.sig.len(), a.entities.len()))
            .collect()
    }

    // ------------------------------------------------------------ resources

    pub fn insert_resource<T: 'static + Send + Sync>(&mut self, r: T) {
        self.resources.insert(TypeId::of::<T>(), Box::new(r));
    }
    pub fn resource<T: 'static + Send + Sync>(&self) -> &T {
        self.try_resource().expect("missing resource")
    }
    pub fn resource_mut<T: 'static + Send + Sync>(&mut self) -> &mut T {
        self.resources
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut())
            .expect("missing resource")
    }
    pub fn try_resource<T: 'static + Send + Sync>(&self) -> Option<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref())
    }
    pub fn remove_resource<T: 'static + Send + Sync>(&mut self) -> Option<T> {
        self.resources
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast::<T>().ok())
            .map(|b| *b)
    }
}

// ---------------------------------------------------------------- commands

/// Deferred structural changes, recorded during iteration, applied after.
#[derive(Default)]
pub struct CommandBuffer {
    ops: Vec<WorldOp>,
}

impl CommandBuffer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn spawn<B: Bundle + Send>(&mut self, bundle: B) {
        self.ops.push(Box::new(move |w| {
            w.spawn(bundle);
        }));
    }
    pub fn despawn(&mut self, e: Entity) {
        self.ops.push(Box::new(move |w| {
            w.despawn(e);
        }));
    }
    pub fn run(&mut self, f: impl FnOnce(&mut World) + Send + 'static) {
        self.ops.push(Box::new(f));
    }
    pub fn apply(&mut self, world: &mut World) {
        for op in self.ops.drain(..) {
            op(world);
        }
    }
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct P(f32, f32);
    #[derive(Debug, PartialEq)]
    struct V(f32, f32);
    struct Tag;

    #[test]
    fn spawn_query_despawn() {
        let mut w = World::new();
        let a = w.spawn((P(0.0, 0.0), V(1.0, 0.0)));
        let b = w.spawn((P(5.0, 5.0), V(0.0, 1.0)));
        let c = w.spawn((P(9.0, 9.0),));
        w.each::<(&mut P, &V)>(|_, (p, v)| {
            p.0 += v.0;
            p.1 += v.1;
        });
        assert_eq!(w.get::<P>(a), Some(&P(1.0, 0.0)));
        assert_eq!(w.get::<P>(b), Some(&P(5.0, 6.0)));
        assert_eq!(w.get::<P>(c), Some(&P(9.0, 9.0)));
        assert!(w.despawn(a));
        assert!(!w.is_alive(a));
        assert_eq!(w.count::<(&P,)>(), 2);
        // b's row moved by swap_remove; access must still be correct
        assert_eq!(w.get::<V>(b), Some(&V(0.0, 1.0)));
    }

    #[test]
    fn insert_remove_migrates() {
        let mut w = World::new();
        let e = w.spawn((P(1.0, 2.0),));
        w.insert(e, V(3.0, 4.0));
        assert_eq!(w.get::<P>(e), Some(&P(1.0, 2.0)));
        assert_eq!(w.get::<V>(e), Some(&V(3.0, 4.0)));
        w.insert(e, Tag);
        w.remove::<V>(e);
        assert_eq!(w.get::<V>(e), None);
        assert_eq!(w.get::<P>(e), Some(&P(1.0, 2.0)));
        assert!(w.has::<Tag>(e));
    }

    #[test]
    fn generations_prevent_stale_access() {
        let mut w = World::new();
        let a = w.spawn((P(0.0, 0.0),));
        w.despawn(a);
        let b = w.spawn((P(7.0, 7.0),));
        assert_eq!(b.index, a.index);
        assert_ne!(b.gen, a.gen);
        assert_eq!(w.get::<P>(a), None);
        assert_eq!(w.get::<P>(b), Some(&P(7.0, 7.0)));
    }
}
