use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

pub struct Context {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|prev| prev.downcast::<T>().ok().map(|b| *b))
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_mut::<T>())
    }

    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast::<T>().ok().map(|b| *b))
    }

    pub fn contains<T: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("entries", &self.map.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Tag(u32);

    struct NotClone(String);

    #[test]
    fn insert_and_get_by_type() {
        let mut ctx = Context::new();
        assert!(ctx.insert(Tag(42)).is_none());
        assert_eq!(ctx.get::<Tag>(), Some(&Tag(42)));
    }

    #[test]
    fn different_types_coexist() {
        let mut ctx = Context::new();
        ctx.insert(Tag(1));
        ctx.insert(String::from("hello"));
        assert_eq!(ctx.get::<Tag>(), Some(&Tag(1)));
        assert_eq!(ctx.get::<String>().map(String::as_str), Some("hello"));
    }

    #[test]
    fn insert_replaces_same_type_and_returns_prev() {
        let mut ctx = Context::new();
        ctx.insert(Tag(1));
        let prev = ctx.insert(Tag(2));
        assert_eq!(prev, Some(Tag(1)));
        assert_eq!(ctx.get::<Tag>(), Some(&Tag(2)));
    }

    #[test]
    fn get_mut_permits_mutation_in_place() {
        let mut ctx = Context::new();
        ctx.insert(Tag(0));
        ctx.get_mut::<Tag>().unwrap().0 = 7;
        assert_eq!(ctx.get::<Tag>(), Some(&Tag(7)));
    }

    #[test]
    fn remove_returns_owned_value_and_clears() {
        let mut ctx = Context::new();
        ctx.insert(Tag(9));
        assert_eq!(ctx.remove::<Tag>(), Some(Tag(9)));
        assert!(!ctx.contains::<Tag>());
        assert!(ctx.get::<Tag>().is_none());
    }

    #[test]
    fn absent_type_returns_none() {
        let ctx = Context::new();
        assert!(ctx.get::<Tag>().is_none());
        assert!(!ctx.contains::<Tag>());
    }

    #[test]
    fn non_clone_types_are_allowed() {
        let mut ctx = Context::new();
        ctx.insert(NotClone("present".into()));
        assert_eq!(ctx.get::<NotClone>().unwrap().0, "present");
    }
}
