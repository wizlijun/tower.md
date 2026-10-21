//! Compile-time extension seam. A kit adds projection groups and business
//! reducers on top of the core; the core never names a kit's concepts.

pub trait Kit: Sync {
    /// Top-level projection groups this kit owns, inserted before core groups.
    fn state_groups(&self) -> &'static [&'static str] {
        &[]
    }
}
