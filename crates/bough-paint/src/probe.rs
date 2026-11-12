//! What of Rust's generics wgsl-rs can carry to WGSL (journal section 2).
#[wgsl_rs::wgsl]
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
