//! What of Rust's generics wgsl-rs can carry to WGSL (journal section 2): a trait method on a concrete type
//! reached through a turbofish is monomorphised; a generic impl is dropped (the test below holds that fact).
#[wgsl_rs::wgsl(skip_validation)]
pub mod p1 {
    use wgsl_rs::std::*;
    pub trait Ex {
        fn eval(u: u32, c: u32) -> u32;
    }
    #[derive(Clone, Copy)]
    pub struct Hover {
        pub pad: u32,
    }
    impl Ex for Hover {
        fn eval(u: u32, c: u32) -> u32 {
            select(1u32, 2u32, u == c)
        }
    }
    #[derive(Clone, Copy)]
    pub struct Twice<C> {
        pub pad: C,
    }
    impl<C: Ex> Ex for Twice<C> {
        fn eval(u: u32, c: u32) -> u32 {
            C::eval(u, c) * 2u32
        }
    }
    pub fn shade<E: Ex>(u: u32, c: u32) -> u32 {
        E::eval(u, c) + 1u32
    }
    pub fn go(u: u32, c: u32) -> u32 {
        shade::<Twice<Hover>>(u, c)
    }
}

#[cfg(test)]
mod tests {
    /// R2: `impl<C: Ex> Ex for Twice<C>` runs as Rust and is absent from the WGSL (the call to `Twice_eval`
    /// has no definition). If this ever fails, wgsl-rs learned generic impls and the const assembly can go.
    #[test]
    fn generic_impl_is_dropped_from_wgsl() {
        assert_eq!(super::p1::go(3, 3), 5);
        let w = super::p1::WGSL_SOURCE.wgsl_source().unwrap();
        assert!(w.contains("Twice_eval(u, c)") && !w.contains("fn Twice_eval"), "{w}");
    }
}
