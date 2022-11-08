pub mod debug;
pub use debug::DebugWithDb;

#[doc(hidden)]
pub mod re_export {
    pub use slotmap;
    pub use incremental;
    pub use string_interner;
}

pub trait Indexed {
    type Storage: Default;
    fn register(_incr: &::incremental::IncrState, _storage: &::std::rc::Rc<::std::cell::RefCell<Self::Storage>>) {
        // default is not to register
    }
}

pub trait ProviderFor<T: Indexed> {
    fn __storage__(&self) -> &::std::cell::RefCell<T::Storage>;
}

use string_interner::{DefaultSymbol, StringInterner};

pub struct InternedString(DefaultSymbol);

impl Indexed for InternedString {
    type Storage = StringInterner;
}

impl InternedString {
    pub fn new(db: &impl ProviderFor<Self>, string: impl AsRef<str>) -> Self {
        let mut interner = db.__storage__().borrow_mut();
        Self(interner.get_or_intern(string.as_ref()))
    }
}

impl<Db: ProviderFor<Self>> DebugWithDb<Db> for InternedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &Db, _include_all_fields: bool) -> std::fmt::Result {
        let storage = db.__storage__().borrow();
        let string = storage.resolve(self.0);
        write!(f, "{:?}", string)
    }
}

#[macro_export]
macro_rules! interned {

    // special case
    ($vis:vis type $id:ident = String;) => {
        #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $id($crate::re_export::string_interner::DefaultSymbol);
        impl $id {
            $vis fn new(db: &impl $crate::ProviderFor<$crate::InternedString>, s: impl AsRef<str>) -> Self {
                let mut storage = db.__storage__().borrow_mut();
                let sym = storage.get_or_intern(s.as_ref());
                $id(sym)
            }
        }
        impl ::std::fmt::Debug for $id {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                let _usize = $crate::re_export::string_interner::Symbol::to_usize(self.0);
                write!(f, "{}({})", stringify!($id), _usize)
            }
        }
        impl<Db: $crate::ProviderFor<$crate::InternedString>> $crate::DebugWithDb<Db> for $id {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>, db: &Db, include_all_fields: bool) -> ::std::fmt::Result {
                let storage = db.__storage__().borrow();
                let string = storage.resolve(self.0).unwrap();
                ::std::fmt::Debug::fmt(&string, f)
            }
        }
    };

    // structs
    (@accept_ty String) => { String };
    (@accept_ty $field_ty:ty) => { $field_ty };
    (@clone $field:ident String) => { $field.clone() };
    (@clone $field:ident $field_ty:ty) => { $field.clone() };

    ($(#[$attr:meta])* $vis:vis struct $id:ident {
        $(
            #[id]
            $id_field_vis:vis $id_field:ident : $id_field_ty:ty,
        )?
        $($field_vis:vis $field:ident : $field_ty:ty,)+
    }) => {
        $(#[$attr])*
        $crate::re_export::slotmap::new_key_type! { $vis struct $id; }
        paste::paste! {
            impl $crate::Indexed for $id {
                type Storage = (
                    // look up by all but the id fields
                    ::std::collections::HashMap<($($field_ty,)*), $id>,
                    // get back a type with all fields on it
                    $crate::re_export::slotmap::SlotMap<$id, [<__ $id Data >]>,
                );
            }
            $vis struct [<__ $id Data >] {
                $($id_field: $id_field_ty,)?
                    $($field: $field_ty,)*
            }
            #[allow(dead_code)]
            impl $id {
                $vis fn new(
                    db: &impl $crate::ProviderFor<Self>,
                    $($field: $crate::interned!(@accept_ty $field_ty),)+
                ) -> Self {
                    let mut storage = db.__storage__().borrow_mut();
                    let (__hashmap, __slotmap) = &mut *storage;
                    let __key = ($($crate::interned!(@clone $field $field_ty),)*);
                    if let Some(id) = __hashmap.get(&__key) {
                        return *id
                    }
                    let __id = __slotmap.insert_with_key(|_k| [<__ $id Data>] {
                        $($id_field: _k,)?
                        $($field: $crate::interned!(@clone $field $field_ty),)*
                    });
                    __hashmap.insert(__key, __id);
                    __id
                }
                $(
                    $field_vis fn $field(&self, db: &impl $crate::ProviderFor<Self>) -> $field_ty {
                        let storage = db.__storage__().borrow();
                        let (_, slotmap) = &*storage;
                        slotmap.get(*self).unwrap().$field.clone()
                    }
                 )*
            }
            impl<_Db: $crate::ProviderFor<$id>> $crate::DebugWithDb<_Db> for $id {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>, _db: &_Db, _include_all_fields: bool) -> ::std::fmt::Result {
                    let storage = _db.__storage__().borrow();
                    let (_, slotmap) = &*storage;
                    use $crate::debug::helper::Fallback;
                    let s = slotmap.get(*self).unwrap();
                    f
                        .debug_struct(stringify!($id))
                        $(.field(
                            stringify!($field),
                            &$crate::debug::helper::SalsaDebug::<
                                $field_ty,
                                &_Db,
                            >::salsa_debug(
                                #[allow(clippy::needless_borrow)]
                                &s.$field,
                                _db,
                                _include_all_fields,
                            ),
                        ))*
                        .finish()
                }
            }
        }
    };
}

#[macro_export]
macro_rules! tracked {
    // structs
    ($(#[$attr:meta])* $vis:vis struct $id:ident {
        $(
            #[id]
            $id_field_vis:vis $id_field:ident : $id_field_ty:ty,
            )?
            $($field_vis:vis $field:ident : $field_ty:ty,)+
    }) => {
        $(#[$attr])*
            $crate::re_export::slotmap::new_key_type! { $vis struct $id; }

        paste::paste! {
            impl $crate::Indexed for $id {
                type Storage = 
                    $crate::re_export::slotmap::SlotMap<$id, [<__ $id Data >]> ;
            }
            $vis struct [<__ $id Data >] {
                $($id_field: $id_field_ty,)?
                    $($field: $field_ty,)*
            }
            #[allow(dead_code)]
            impl $id {
                $vis fn new(__db: &impl $crate::ProviderFor<Self>, $($field: $field_ty,)+) -> Self {
                    let slotmap = __db.__storage__();
                    let mut slotmap_ = slotmap.borrow_mut();
                    slotmap_.insert_with_key(|_k| [<__ $id Data>] {
                        $($id_field: _k,)?
                            $($field,)+
                    })
                }
                $(
                    $field_vis fn $field(&self, __db: &impl $crate::ProviderFor<Self>) -> $field_ty {
                        let slotmap = __db.__storage__();
                        let slotmap = slotmap.borrow();
                        slotmap.get(*self).unwrap().$field.clone()
                    }
                 )*
            }
        }
    };

    // functions
    ($vis:vis fn $function:ident($db:ident : &$db_ty:ty, $($arg:ident : $arg_ty:ty,)+) -> Incr<$r:ty> {
        $($body:tt)*
    }) => {
        ::paste::paste! {
            $vis struct [<$function:camel>];
            impl $crate::Indexed for [<$function:camel>] {
                type Storage = $crate::re_export::incremental::WeakHashMap< ($($arg_ty,)*), $r >;
                fn register(incr: &IncrState, storage: &::std::rc::Rc<::std::cell::RefCell<Self::Storage>>) {
                    incr.add_weak_map(storage.clone());
                }
            }
            #[allow(dead_code)]
            impl [<$function:camel>] {
                fn get($db: &$db_ty, $($arg : $arg_ty,)*) -> $crate::re_export::incremental::Incr<$r> {
                    fn __fn($db: &$db_ty, $($arg: $arg_ty,)*) -> $crate::re_export::incremental::Incr<$r> {
                        $($body)*
                    }
                    let storage = <$db_ty as $crate::ProviderFor<[<$function:camel>]>>::__storage__($db);
                    let mut storage_ = storage.borrow_mut();
                    let entry = storage_.entry(($($arg.clone(),)*));
                    let execute = || {
                        __fn($db, $($arg,)*)
                    };

                    match entry {
                        ::std::collections::hash_map::Entry::Occupied(mut occ) => {
                            let incr = occ.get().upgrade();
                            if let Some(i) = incr {
                                return i
                            } else {
                                let val = execute();
                                occ.insert(val.weak());
                                return val
                            }
                        }
                        ::std::collections::hash_map::Entry::Vacant(vacant) => {
                            let val = execute();
                            vacant.insert(val.weak());
                            return val
                        }
                    }
                }
            }
            $vis fn $function($db: &$db_ty, $($arg: $arg_ty,)*) -> $crate::re_export::incremental::Incr<$r> {
                [<$function:camel>]::get(&$db, $($arg,)*)
            }
        }
    }
}

#[macro_export]
macro_rules! db {
    ($vis:vis struct $Db:ident {
            $($input:ident: $input_ty:ty),* $(,)?
    }) => {
        $vis struct $Db {
            incr: $crate::re_export::incremental::IncrState,
            $(
                $input:
                ::std::rc::Rc<::std::cell::RefCell<<$input_ty as $crate::Indexed>::Storage>>,
            )*
        }

        impl $Db {
            $vis fn new() -> Self {
                let __state = $crate::re_export::incremental::IncrState::new();
                $(
                    let $input = ::std::rc::Rc::new(::std::cell::RefCell::new(
                        <$input_ty as $crate::Indexed>::Storage::default()
                    ));
                    <$input_ty as $crate::Indexed>::register(&__state, &&&$input);
                )*
                Self {
                    incr: __state,
                    $($input,)*
                }
            }
        }

        impl ::core::ops::Deref for $Db {
            type Target = $crate::re_export::incremental::IncrState;
            fn deref(&self) -> &Self::Target {
                &self.incr
            }
        }

        $(
            impl $crate::ProviderFor<$input_ty> for $Db {
                fn __storage__(&self) -> &::std::cell::RefCell<<$input_ty as $crate::Indexed>::Storage> {
                    &self.$input
                }
            }
        )*
    };
}
