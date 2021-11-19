#[cfg(feature = "stdvec")]
mod vec_builder;
#[cfg(feature = "smallvec")]
mod small_vec_builder;

#[cfg(feature = "stdvec")]
pub use vec_builder::InPlaceVecBuilder;
#[cfg(feature = "smallvec")]
pub use small_vec_builder::InPlaceSmallVecBuilder;